use crate::remote::status::RemoteAgentStatus;
use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary, RemoteUserSummary};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDiagnosticStep {
    pub key: &'static str,
    pub title: &'static str,
    pub status: &'static str,
    pub severity: &'static str,
    pub message: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RemoteDiagnosticReport {
    pub scope: &'static str,
    pub overall: &'static str,
    pub summary: &'static str,
    pub started_at: String,
    pub finished_at: String,
    pub steps: Vec<RemoteDiagnosticStep>,
}

pub fn diagnose_remote_access(
    config: &RemoteConfig,
    has_credential: bool,
    status: &RemoteAgentStatus,
    now: chrono::DateTime<chrono::Utc>,
) -> RemoteDiagnosticReport {
    let mut steps = Vec::new();

    steps.push(step(
        "config",
        "remoteDiagnosticsStepConfig",
        config.remote_access_enabled,
        "error",
        Some("remoteDiagnosticsMessageDisabled"),
    ));
    steps.push(step(
        "binding",
        "remoteDiagnosticsStepBinding",
        config.user.is_some() && config.device.is_some(),
        "error",
        Some("remoteDiagnosticsMessageNotBound"),
    ));
    steps.push(step(
        "credential",
        "remoteDiagnosticsStepCredential",
        has_credential,
        "error",
        Some("remoteDiagnosticsMessageMissingCredential"),
    ));
    steps.push(step(
        "remote_control",
        "remoteDiagnosticsStepRemoteControl",
        config.remote_control_enabled,
        "warning",
        Some("remoteDiagnosticsMessageRemoteControlDisabled"),
    ));
    steps.push(step(
        "device_socket",
        "remoteDiagnosticsStepDeviceSocket",
        status.state == "online" || status.state == "connecting" || status.state == "reconnecting",
        "warning",
        Some("remoteDiagnosticsMessageServerUnreachable"),
    ));

    let has_error = steps
        .iter()
        .any(|item| item.status == "failed" && item.severity == "error");
    let has_warning = steps.iter().any(|item| item.status == "failed");
    let overall = if has_error {
        "failed"
    } else if has_warning {
        "degraded"
    } else {
        "passed"
    };
    let summary = match overall {
        "passed" => "remoteDiagnosticsSummaryPassed",
        "failed" => "remoteDiagnosticsSummaryFailed",
        _ => "remoteDiagnosticsSummaryDegraded",
    };
    let timestamp = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

    RemoteDiagnosticReport {
        scope: "local_agent",
        overall,
        summary,
        started_at: timestamp.clone(),
        finished_at: timestamp,
        steps,
    }
}

fn step(
    key: &'static str,
    title: &'static str,
    passed: bool,
    severity: &'static str,
    message: Option<&'static str>,
) -> RemoteDiagnosticStep {
    RemoteDiagnosticStep {
        key,
        title,
        status: if passed { "passed" } else { "failed" },
        severity: if passed { "info" } else { severity },
        message: if passed { None } else { message },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::agent_state::RemoteAgentState;

    fn bound_config() -> RemoteConfig {
        let mut config = RemoteConfig::default_for_server("https://remote.example.com");
        config.user = Some(RemoteUserSummary {
            id: "user_1".to_string(),
            email: "user@example.com".to_string(),
            role: "owner".to_string(),
        });
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "Desk Mac".to_string(),
        });
        config
    }

    #[test]
    fn reports_passed_when_local_agent_is_ready() {
        let status = RemoteAgentStatus::new(RemoteAgentState::Online);
        let report = diagnose_remote_access(
            &bound_config(),
            true,
            &status,
            chrono::DateTime::parse_from_rfc3339("2026-06-30T00:00:00Z")
                .unwrap()
                .with_timezone(&chrono::Utc),
        );

        assert_eq!(report.overall, "passed");
        assert_eq!(report.summary, "remoteDiagnosticsSummaryPassed");
        assert!(report.steps.iter().all(|step| step.status == "passed"));
    }

    #[test]
    fn reports_failed_when_binding_or_credential_is_missing() {
        let config = RemoteConfig::default_for_server("https://remote.example.com");
        let status = RemoteAgentStatus::new(RemoteAgentState::Online);
        let report = diagnose_remote_access(&config, false, &status, chrono::Utc::now());

        assert_eq!(report.overall, "failed");
        assert!(report
            .steps
            .iter()
            .any(|step| step.key == "binding" && step.message == Some("remoteDiagnosticsMessageNotBound")));
        assert!(report
            .steps
            .iter()
            .any(|step| step.key == "credential" && step.message == Some("remoteDiagnosticsMessageMissingCredential")));
    }
}
