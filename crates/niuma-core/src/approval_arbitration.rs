use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

use crate::models::NiumaEvent;

pub const WATCHER_APPROVAL_DELAY: Duration = Duration::from_secs(2);
pub const ACTIVE_TTL: chrono::Duration = chrono::Duration::minutes(10);
pub const INACTIVE_TTL: chrono::Duration = chrono::Duration::minutes(5);
pub const RECENT_EMISSION_TTL: chrono::Duration = chrono::Duration::seconds(10);
pub const ACTIVE_MISSED_THRESHOLD: u32 = 2;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ApprovalScope {
    pub project_path: String,
}

impl ApprovalScope {
    pub fn new(project_path: impl Into<String>) -> Self {
        Self {
            project_path: project_path.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalFingerprintBasis {
    Command,
    Description,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ApprovalFingerprint {
    pub key: String,
    pub project_path: String,
    pub session_id: Option<String>,
    pub basis: ApprovalFingerprintBasis,
}

impl ApprovalFingerprint {
    pub fn from_parts(
        project_path: &str,
        session_id: Option<&str>,
        command: Option<&str>,
        description: Option<&str>,
    ) -> Option<Self> {
        let project_path = project_path.trim().to_string();
        let session_id = session_id
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToString::to_string);
        let (basis, text) = normalize_fingerprint_text(command)
            .map(|value| (ApprovalFingerprintBasis::Command, value))
            .or_else(|| {
                normalize_fingerprint_text(description)
                    .map(|value| (ApprovalFingerprintBasis::Description, value))
            })?;
        let session_part = session_id.as_deref().unwrap_or("");
        let key = format!(
            "codex-approval:{}:{}:{}:{}",
            stable_hash(&project_path),
            stable_hash(session_part),
            match basis {
                ApprovalFingerprintBasis::Command => "command",
                ApprovalFingerprintBasis::Description => "description",
            },
            stable_hash(&text)
        );
        Some(Self {
            key,
            project_path,
            session_id,
            basis,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HookCapability {
    Unknown,
    Active {
        last_seen_at: DateTime<Utc>,
        missed_count: u32,
    },
    Inactive {
        last_fallback_at: DateTime<Utc>,
    },
}

#[derive(Clone, Debug)]
pub struct PendingWatcherApproval {
    pub fingerprint: ApprovalFingerprint,
    pub event: NiumaEvent,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone, Debug)]
pub struct RecentApprovalEmission {
    pub fingerprint: ApprovalFingerprint,
    pub source: ApprovalEmissionSource,
    pub emitted_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ApprovalEmissionSource {
    Hook,
    Watcher,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WatcherApprovalDecision {
    Delay(Duration),
    EmitNow(NiumaEvent),
    Suppress,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HookApprovalDecision {
    AcceptHook,
    ReturnToCodex,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExpiredWatcherApproval {
    Emit(NiumaEvent),
}

#[derive(Default)]
pub struct ApprovalArbiter {
    capabilities: HashMap<ApprovalScope, HookCapability>,
    pending_candidates: HashMap<String, PendingWatcherApproval>,
    recent_emissions: Vec<RecentApprovalEmission>,
}

impl ApprovalArbiter {
    pub fn on_watcher_approval(
        &mut self,
        fingerprint: ApprovalFingerprint,
        event: NiumaEvent,
        now: DateTime<Utc>,
    ) -> WatcherApprovalDecision {
        self.prune_recent(now);
        // hook 已经接管同一个授权时，迟到的 watcher fallback 只会制造重复通知。
        if self.recent_emissions.iter().any(|emission| {
            emission.fingerprint.key == fingerprint.key
                && emission.source == ApprovalEmissionSource::Hook
        }) {
            return WatcherApprovalDecision::Suppress;
        }
        let scope = ApprovalScope::new(fingerprint.project_path.clone());
        match self.capability_for_scope(&scope, now) {
            HookCapability::Inactive { .. } => {
                self.record_emission(&fingerprint, ApprovalEmissionSource::Watcher, now);
                WatcherApprovalDecision::EmitNow(event)
            }
            HookCapability::Unknown | HookCapability::Active { .. } => {
                self.pending_candidates.insert(
                    fingerprint.key.clone(),
                    PendingWatcherApproval {
                        fingerprint,
                        event,
                        created_at: now,
                    },
                );
                WatcherApprovalDecision::Delay(WATCHER_APPROVAL_DELAY)
            }
        }
    }

    pub fn on_hook_approval(
        &mut self,
        fingerprint: ApprovalFingerprint,
        now: DateTime<Utc>,
    ) -> HookApprovalDecision {
        self.prune_recent(now);
        self.pending_candidates.remove(&fingerprint.key);
        let was_watcher_fallback = self.recent_emissions.iter().any(|emission| {
            emission.fingerprint.key == fingerprint.key
                && emission.source == ApprovalEmissionSource::Watcher
        });
        self.capabilities.insert(
            ApprovalScope::new(fingerprint.project_path.clone()),
            HookCapability::Active {
                last_seen_at: now,
                missed_count: 0,
            },
        );
        if was_watcher_fallback {
            HookApprovalDecision::ReturnToCodex
        } else {
            self.record_emission(&fingerprint, ApprovalEmissionSource::Hook, now);
            HookApprovalDecision::AcceptHook
        }
    }

    pub fn expire_candidate(
        &mut self,
        fingerprint: &ApprovalFingerprint,
        now: DateTime<Utc>,
    ) -> Option<ExpiredWatcherApproval> {
        self.prune_recent(now);
        let pending = self.pending_candidates.remove(&fingerprint.key)?;
        let scope = ApprovalScope::new(fingerprint.project_path.clone());
        let next_capability = match self.capability_for_scope(&scope, now) {
            HookCapability::Active {
                last_seen_at,
                missed_count,
            } => {
                let missed_count = missed_count + 1;
                if missed_count >= ACTIVE_MISSED_THRESHOLD {
                    HookCapability::Inactive {
                        last_fallback_at: now,
                    }
                } else {
                    HookCapability::Active {
                        last_seen_at,
                        missed_count,
                    }
                }
            }
            HookCapability::Unknown | HookCapability::Inactive { .. } => HookCapability::Inactive {
                last_fallback_at: now,
            },
        };
        self.capabilities.insert(scope, next_capability);
        self.record_emission(fingerprint, ApprovalEmissionSource::Watcher, now);
        Some(ExpiredWatcherApproval::Emit(pending.event))
    }

    fn capability_for_scope(
        &mut self,
        scope: &ApprovalScope,
        now: DateTime<Utc>,
    ) -> HookCapability {
        let capability = self
            .capabilities
            .get(scope)
            .cloned()
            .unwrap_or(HookCapability::Unknown);
        let effective = match capability {
            HookCapability::Active { last_seen_at, .. } if now - last_seen_at > ACTIVE_TTL => {
                HookCapability::Unknown
            }
            HookCapability::Inactive { last_fallback_at }
                if now - last_fallback_at > INACTIVE_TTL =>
            {
                HookCapability::Unknown
            }
            other => other,
        };
        self.capabilities.insert(scope.clone(), effective.clone());
        effective
    }

    fn record_emission(
        &mut self,
        fingerprint: &ApprovalFingerprint,
        source: ApprovalEmissionSource,
        now: DateTime<Utc>,
    ) {
        self.recent_emissions.push(RecentApprovalEmission {
            fingerprint: fingerprint.clone(),
            source,
            emitted_at: now,
        });
    }

    fn prune_recent(&mut self, now: DateTime<Utc>) {
        self.recent_emissions
            .retain(|emission| now - emission.emitted_at <= RECENT_EMISSION_TTL);
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.pending_candidates.len()
    }

    #[cfg(test)]
    fn mark_inactive_for_test(&mut self, fingerprint: &ApprovalFingerprint, now: DateTime<Utc>) {
        self.capabilities.insert(
            ApprovalScope::new(fingerprint.project_path.clone()),
            HookCapability::Inactive {
                last_fallback_at: now,
            },
        );
    }

    #[cfg(test)]
    fn is_inactive_for_test(
        &mut self,
        fingerprint: &ApprovalFingerprint,
        now: DateTime<Utc>,
    ) -> bool {
        matches!(
            self.capability_for_scope(&ApprovalScope::new(fingerprint.project_path.clone()), now),
            HookCapability::Inactive { .. }
        )
    }
}

fn normalize_fingerprint_text(value: Option<&str>) -> Option<String> {
    let normalized = value?.split_whitespace().collect::<Vec<_>>().join(" ");
    (!normalized.is_empty()).then_some(normalized)
}

fn stable_hash(text: &str) -> String {
    let mut hash: u64 = 14_695_981_039_346_656_037;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    format!("{hash:x}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn now() -> DateTime<Utc> {
        Utc.timestamp_opt(1_000, 0).single().unwrap()
    }

    fn fingerprint() -> ApprovalFingerprint {
        ApprovalFingerprint::from_parts("/tmp/demo", Some("session-1"), Some("cargo test"), None)
            .unwrap()
    }

    fn event(id: &str) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: format!("dedupe-{id}"),
            source: "codex-session-file".to_string(),
            tool: crate::models::ToolKind::Codex,
            session_id: "session-1".to_string(),
            parent_session_id: None,
            normalized_session_id: None,
            session_scope: None,
            agent_nickname: None,
            agent_role: None,
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            event_type: crate::models::EventType::ApprovalRequested,
            severity: "urgent".to_string(),
            summary: "exec_command: cargo test".to_string(),
            content: Some("exec_command: cargo test".to_string()),
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            created_at: now(),
        }
    }

    #[test]
    fn approval_fingerprint_prefers_command_over_description() {
        let fingerprint = ApprovalFingerprint::from_parts(
            "/tmp/demo",
            Some("session-1"),
            Some("  cargo   test  "),
            Some("运行测试"),
        )
        .unwrap();

        assert_eq!(fingerprint.project_path, "/tmp/demo");
        assert_eq!(fingerprint.session_id.as_deref(), Some("session-1"));
        assert_eq!(fingerprint.basis, ApprovalFingerprintBasis::Command);
        assert_eq!(
            fingerprint.key,
            ApprovalFingerprint::from_parts(
                "/tmp/demo",
                Some("session-1"),
                Some("cargo test"),
                None,
            )
            .unwrap()
            .key
        );
    }

    #[test]
    fn approval_fingerprint_uses_description_when_command_is_missing() {
        let fingerprint = ApprovalFingerprint::from_parts(
            "/tmp/demo",
            None,
            None,
            Some(" 是否允许执行这个命令？ "),
        )
        .unwrap();

        assert_eq!(fingerprint.session_id, None);
        assert_eq!(fingerprint.basis, ApprovalFingerprintBasis::Description);
    }

    #[test]
    fn approval_fingerprint_is_none_without_command_or_description() {
        let fingerprint = ApprovalFingerprint::from_parts("/tmp/demo", Some("s1"), None, None);

        assert!(fingerprint.is_none());
    }

    #[test]
    fn watcher_unknown_creates_delayed_candidate() {
        let mut arbiter = ApprovalArbiter::default();
        let decision = arbiter.on_watcher_approval(fingerprint(), event("watcher-1"), now());

        assert_eq!(
            decision,
            WatcherApprovalDecision::Delay(WATCHER_APPROVAL_DELAY)
        );
        assert_eq!(arbiter.pending_count(), 1);
    }

    #[test]
    fn watcher_inactive_emits_immediately() {
        let mut arbiter = ApprovalArbiter::default();
        let fp = fingerprint();
        arbiter.mark_inactive_for_test(&fp, now());

        let decision = arbiter.on_watcher_approval(fp, event("watcher-1"), now());

        assert!(matches!(decision, WatcherApprovalDecision::EmitNow(_)));
    }

    #[test]
    fn hook_suppresses_pending_watcher_candidate() {
        let mut arbiter = ApprovalArbiter::default();
        let fp = fingerprint();
        arbiter.on_watcher_approval(fp.clone(), event("watcher-1"), now());

        let decision = arbiter.on_hook_approval(fp, now());

        assert_eq!(decision, HookApprovalDecision::AcceptHook);
        assert_eq!(arbiter.pending_count(), 0);
    }

    #[test]
    fn expiring_unknown_candidate_emits_watcher_and_marks_inactive() {
        let mut arbiter = ApprovalArbiter::default();
        let fp = fingerprint();
        arbiter.on_watcher_approval(fp.clone(), event("watcher-1"), now());

        let expired = arbiter.expire_candidate(&fp, now() + chrono::Duration::seconds(2));

        assert!(matches!(expired, Some(ExpiredWatcherApproval::Emit(_))));
        assert!(arbiter.is_inactive_for_test(&fp, now() + chrono::Duration::seconds(2)));
    }

    #[test]
    fn late_hook_after_watcher_fallback_returns_to_codex() {
        let mut arbiter = ApprovalArbiter::default();
        let fp = fingerprint();
        arbiter.on_watcher_approval(fp.clone(), event("watcher-1"), now());
        arbiter.expire_candidate(&fp, now() + chrono::Duration::seconds(2));

        let decision = arbiter.on_hook_approval(fp, now() + chrono::Duration::seconds(3));

        assert_eq!(decision, HookApprovalDecision::ReturnToCodex);
    }

    #[test]
    fn watcher_after_hook_emission_is_suppressed() {
        let mut arbiter = ApprovalArbiter::default();
        let fp = fingerprint();
        assert_eq!(
            arbiter.on_hook_approval(fp.clone(), now()),
            HookApprovalDecision::AcceptHook
        );

        let decision = arbiter.on_watcher_approval(
            fp,
            event("watcher-after-hook"),
            now() + chrono::Duration::seconds(1),
        );

        assert_eq!(decision, WatcherApprovalDecision::Suppress);
        assert_eq!(arbiter.pending_count(), 0);
    }
}
