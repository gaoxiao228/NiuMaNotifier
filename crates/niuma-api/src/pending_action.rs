use chrono::{DateTime, Utc};
use niuma_core::models::{
    AttentionItem, EventInteractionDetail, EventInteractionHandling, EventInteractionKind,
    EventInteractionQuestion, NiumaEvent, RuntimeStateStatus,
};
use niuma_core::tool_session::{
    PendingAction, PendingActionButton, PendingActionType, PendingInputField,
    PendingInputFieldType, PendingInputOption, SubmitSpec, ToolSessionDetail, ToolSessionScope,
};
use serde_json::json;

use crate::state::AppState;

struct PendingActionCandidate {
    priority: usize,
    created_at: DateTime<Utc>,
    action: PendingAction,
}

pub(crate) fn pending_action_for_session(
    state: &AppState,
    detail: &ToolSessionDetail,
) -> Result<Option<PendingAction>, String> {
    let input = state.store.main_state_input()?;
    let mut candidates = input
        .state
        .attention_items
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                RuntimeStateStatus::WaitingApproval | RuntimeStateStatus::WaitingInput
            )
        })
        .filter_map(|item| candidate_for_attention_item(&input.public_events, item, detail))
        .collect::<Vec<_>>();

    candidates.sort_by(|left, right| {
        right
            .priority
            .cmp(&left.priority)
            .then_with(|| left.created_at.cmp(&right.created_at))
    });

    Ok(candidates
        .into_iter()
        .next()
        .map(|candidate| candidate.action))
}

fn candidate_for_attention_item(
    events: &[NiumaEvent],
    item: &AttentionItem,
    detail: &ToolSessionDetail,
) -> Option<PendingActionCandidate> {
    let event = events.iter().find(|event| event.id == item.event_id);
    match event {
        Some(event) if event_matches_detail(event, detail) => {
            candidate_from_event(item, event, detail)
        }
        Some(_) => None,
        None if item_matches_detail(item, detail) => Some(fallback_candidate_from_attention(item)),
        None => None,
    }
}

fn event_matches_detail(event: &NiumaEvent, detail: &ToolSessionDetail) -> bool {
    if event.tool != detail.tool {
        return false;
    }
    if event.session_id == detail.session_id {
        return true;
    }
    if event.parent_session_id.as_deref() == Some(detail.session_id.as_str()) {
        return true;
    }
    detail_allows_normalized_event_match(detail)
        && detail
            .normalized_session_id
            .as_deref()
            .is_some_and(|id| event.normalized_session_id.as_deref() == Some(id))
}

fn detail_allows_normalized_event_match(detail: &ToolSessionDetail) -> bool {
    detail
        .normalized_session_id
        .as_deref()
        .is_some_and(|id| detail.session_id == id)
        || detail.session_scope == Some(ToolSessionScope::Main)
}

fn item_matches_detail(item: &AttentionItem, detail: &ToolSessionDetail) -> bool {
    item.tool == detail.tool && item.session_id == detail.session_id
}

fn candidate_from_event(
    item: &AttentionItem,
    event: &NiumaEvent,
    detail: &ToolSessionDetail,
) -> Option<PendingActionCandidate> {
    let interaction = event.interaction.as_ref();
    let action_type = match item.status {
        RuntimeStateStatus::WaitingApproval => PendingActionType::Approval,
        RuntimeStateStatus::WaitingInput => PendingActionType::Input,
        _ => return None,
    };
    let priority = pending_priority(&action_type, interaction);
    let action = match action_type {
        PendingActionType::Approval => approval_action(item, event, interaction),
        PendingActionType::Input => input_action(item, event, interaction, detail),
    };
    Some(PendingActionCandidate {
        priority,
        created_at: item.created_at,
        action,
    })
}

fn fallback_candidate_from_attention(item: &AttentionItem) -> PendingActionCandidate {
    let action_type = match item.status {
        RuntimeStateStatus::WaitingApproval => PendingActionType::Approval,
        RuntimeStateStatus::WaitingInput => PendingActionType::Input,
        _ => PendingActionType::Approval,
    };
    PendingActionCandidate {
        priority: match action_type {
            PendingActionType::Approval => 20,
            PendingActionType::Input => 10,
        },
        created_at: item.created_at,
        action: PendingAction {
            title: title_for(&action_type).to_string(),
            action_type,
            description: item.summary.clone(),
            actionable: false,
            created_at: item.created_at,
            source_event_id: Some(item.event_id.clone()),
            actions: Vec::new(),
            fields: Vec::new(),
            submit: None,
        },
    }
}

fn pending_priority(
    action_type: &PendingActionType,
    interaction: Option<&EventInteractionDetail>,
) -> usize {
    let niuma_actionable = interaction.is_some_and(|interaction| {
        interaction_kind_matches(action_type, interaction)
            && interaction.handling == EventInteractionHandling::Niuma
            && interaction.actionable
    });
    match (action_type, niuma_actionable) {
        (PendingActionType::Approval, true) => 40,
        (PendingActionType::Input, true) => 30,
        (PendingActionType::Approval, false) => 20,
        (PendingActionType::Input, false) => 10,
    }
}

fn interaction_kind_matches(
    action_type: &PendingActionType,
    interaction: &EventInteractionDetail,
) -> bool {
    matches!(
        (action_type, &interaction.kind),
        (PendingActionType::Approval, EventInteractionKind::Approval)
            | (PendingActionType::Input, EventInteractionKind::Input)
    )
}

fn title_for(action_type: &PendingActionType) -> &'static str {
    match action_type {
        PendingActionType::Approval => "需要授权",
        PendingActionType::Input => "等待输入",
    }
}

fn description_for(event: &NiumaEvent, interaction: Option<&EventInteractionDetail>) -> String {
    interaction
        .and_then(|interaction| interaction.message.clone())
        .or_else(|| event.content.clone())
        .unwrap_or_else(|| event.summary.clone())
}

fn approval_action(
    item: &AttentionItem,
    event: &NiumaEvent,
    interaction: Option<&EventInteractionDetail>,
) -> PendingAction {
    let request_id = interaction.and_then(|interaction| interaction.request_id.clone());
    let actionable = interaction.is_some_and(|interaction| {
        interaction.kind == EventInteractionKind::Approval
            && interaction.handling == EventInteractionHandling::Niuma
            && interaction.actionable
            && interaction.request_id.is_some()
    });
    let actions = if actionable {
        request_id
            .as_deref()
            .map(|request_id| {
                vec![
                    approval_button("allow", "允许", request_id),
                    approval_button("deny", "拒绝", request_id),
                ]
            })
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    PendingAction {
        action_type: PendingActionType::Approval,
        title: "需要授权".to_string(),
        description: description_for(event, interaction),
        actionable: !actions.is_empty(),
        created_at: item.created_at,
        source_event_id: Some(event.id.clone()),
        actions,
        fields: Vec::new(),
        submit: None,
    }
}

fn approval_button(id: &str, label: &str, request_id: &str) -> PendingActionButton {
    PendingActionButton {
        id: id.to_string(),
        label: label.to_string(),
        submit: SubmitSpec {
            method: "POST".to_string(),
            url: "/api/v1/approval-decisions".to_string(),
            body: json!({
                "request_id": request_id,
                "decision": id
            }),
        },
    }
}

fn input_action(
    item: &AttentionItem,
    event: &NiumaEvent,
    interaction: Option<&EventInteractionDetail>,
    detail: &ToolSessionDetail,
) -> PendingAction {
    let input_interaction =
        interaction.filter(|interaction| interaction.kind == EventInteractionKind::Input);
    let fields = input_interaction
        .and_then(|interaction| interaction.schema.as_ref())
        .map(|schema| {
            schema
                .questions
                .iter()
                .map(input_field_from_question)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let niuma_input = input_interaction.filter(|interaction| {
        interaction.handling == EventInteractionHandling::Niuma && interaction.actionable
    });
    let request_id = niuma_input.and_then(|interaction| interaction.request_id.clone());
    let channel_id = niuma_input
        .and_then(|interaction| interaction.control_ref.as_ref())
        .map(|control_ref| control_ref.channel_id.clone());
    let submit = match (request_id, channel_id) {
        (Some(request_id), Some(channel_id)) => Some(SubmitSpec {
            method: "POST".to_string(),
            url: "/api/v1/session-control/answer-input".to_string(),
            body: json!({
                "tool": detail.tool.clone(),
                "session_id": detail.session_id,
                "channel_id": channel_id,
                "request_id": request_id
            }),
        }),
        _ => None,
    };

    PendingAction {
        action_type: PendingActionType::Input,
        title: "等待输入".to_string(),
        description: description_for(event, interaction),
        actionable: submit.is_some(),
        created_at: item.created_at,
        source_event_id: Some(event.id.clone()),
        actions: Vec::new(),
        fields,
        submit,
    }
}

fn input_field_from_question(question: &EventInteractionQuestion) -> PendingInputField {
    PendingInputField {
        id: question.id.clone(),
        label: question
            .header
            .clone()
            .unwrap_or_else(|| question.question.clone()),
        question: question.question.clone(),
        field_type: if question.options.is_empty() {
            PendingInputFieldType::Text
        } else {
            PendingInputFieldType::SingleSelect
        },
        required: true,
        options: question
            .options
            .iter()
            .map(|option| PendingInputOption {
                label: option.label.clone(),
                description: option.description.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use chrono::{TimeZone, Utc};
    use niuma_core::approval_arbitration::ApprovalArbiter;
    use niuma_core::models::{
        ApprovalControlRef, AttentionItem, EventInteractionDetail, EventInteractionHandling,
        EventInteractionKind, EventInteractionOption, EventInteractionQuestion,
        EventInteractionSchema, EventType, NiumaEvent, RuntimeStateStatus, ToolKind,
    };
    use niuma_core::runtime_event::RuntimeEventBus;
    use niuma_core::state_mutation::StateMutationService;
    use niuma_core::store::NiumaStore;
    use niuma_core::tool_session::{
        PendingActionType, PendingInputFieldType, ToolSessionDetail, ToolSessionScope,
    };
    use serde_json::json;

    use crate::pending_action::{fallback_candidate_from_attention, pending_action_for_session};
    use crate::sse::MainStateBroadcaster;
    use crate::state::AppState;
    use crate::tool_sessions::ToolSessionRegistry;

    #[test]
    fn pending_action_prefers_niuma_approval_over_input_and_builds_buttons() {
        let state = test_state("pending_action_prefers_niuma_approval");
        let input_event = input_event("input-1", "session-1", 10);
        let approval_event = approval_event("approval-1", "session-1", 20);
        state
            .store
            .append_events(vec![input_event, approval_event])
            .unwrap();

        let action = pending_action_for_session(&state, &detail("session-1"))
            .unwrap()
            .unwrap();

        assert_eq!(action.action_type, PendingActionType::Approval);
        assert_eq!(action.title, "需要授权");
        assert_eq!(action.description, "请批准 cargo test");
        assert!(action.actionable);
        assert_eq!(action.source_event_id.as_deref(), Some("approval-1"));
        assert_eq!(action.actions.len(), 2);
        assert_eq!(action.actions[0].id, "allow");
        assert_eq!(action.actions[0].label, "允许");
        assert_eq!(
            action.actions[0].submit.body,
            json!({"request_id": "approval-request-1", "decision": "allow"})
        );
        assert_eq!(action.actions[1].id, "deny");
    }

    #[test]
    fn pending_action_builds_input_fields_and_submit_spec() {
        let state = test_state("pending_action_builds_input");
        state
            .store
            .append_event(input_event("input-2", "session-1", 10))
            .unwrap();

        let action = pending_action_for_session(&state, &detail("session-1"))
            .unwrap()
            .unwrap();

        assert_eq!(action.action_type, PendingActionType::Input);
        assert_eq!(action.title, "等待输入");
        assert_eq!(action.description, "请选择运行方式");
        assert!(action.actionable);
        assert!(action.actions.is_empty());
        assert_eq!(action.fields.len(), 2);
        assert_eq!(
            action.fields[0].field_type,
            PendingInputFieldType::SingleSelect
        );
        assert_eq!(action.fields[0].label, "运行形态");
        assert_eq!(action.fields[0].options[0].label, "托盘常驻");
        assert_eq!(action.fields[1].field_type, PendingInputFieldType::Text);
        assert_eq!(
            action.submit.unwrap().body,
            json!({
                "tool": "codex",
                "session_id": "session-1",
                "channel_id": "channel-1",
                "request_id": "input-request-1"
            })
        );
    }

    #[test]
    fn pending_action_keeps_tool_input_schema_fields_without_submit() {
        let state = test_state("pending_action_tool_input_fields");
        let mut event = input_event("input-tool", "session-1", 10);
        if let Some(interaction) = event.interaction.as_mut() {
            interaction.handling = EventInteractionHandling::Tool;
            interaction.actionable = false;
            interaction.request_id = None;
            interaction.control_ref = None;
        }
        state.store.append_event(event).unwrap();

        let action = pending_action_for_session(&state, &detail("session-1"))
            .unwrap()
            .unwrap();

        assert_eq!(action.action_type, PendingActionType::Input);
        assert!(!action.actionable);
        assert!(action.submit.is_none());
        assert_eq!(action.fields.len(), 2);
        assert_eq!(action.fields[0].label, "运行形态");
    }

    #[test]
    fn pending_action_uses_attention_fallback_when_event_missing() {
        let created_at = Utc.timestamp_opt(10, 0).single().unwrap();
        let item = AttentionItem {
            event_id: "missing-event".to_string(),
            tool: ToolKind::Codex,
            session_id: "session-1".to_string(),
            status: RuntimeStateStatus::WaitingApproval,
            summary: "工具中仍有授权等待".to_string(),
            attention_resolve_key: None,
            created_at,
        };

        let action = fallback_candidate_from_attention(&item).action;

        assert_eq!(action.action_type, PendingActionType::Approval);
        assert_eq!(action.description, "工具中仍有授权等待");
        assert!(!action.actionable);
        assert_eq!(action.created_at, created_at);
        assert_eq!(action.source_event_id.as_deref(), Some("missing-event"));
    }

    #[test]
    fn pending_action_matches_normalized_or_parent_session() {
        let state = test_state("pending_action_normalized_parent");
        let mut normalized = approval_event("approval-normalized", "raw-child", 10);
        normalized.normalized_session_id = Some("main-session".to_string());
        let mut parent = input_event("input-parent", "child-session", 20);
        parent.parent_session_id = Some("main-session".to_string());
        state.store.append_events(vec![parent, normalized]).unwrap();

        let mut session_detail = detail("main-session");
        session_detail.normalized_session_id = Some("main-session".to_string());
        let action = pending_action_for_session(&state, &session_detail)
            .unwrap()
            .unwrap();

        assert_eq!(action.action_type, PendingActionType::Approval);
        assert_eq!(
            action.source_event_id.as_deref(),
            Some("approval-normalized")
        );
    }

    #[test]
    fn pending_action_does_not_match_sibling_raw_session_by_normalized_id() {
        let state = test_state("pending_action_sibling_raw_session");
        let mut sibling = approval_event("approval-sibling", "raw-sibling", 10);
        sibling.normalized_session_id = Some("main-session".to_string());
        state.store.append_event(sibling).unwrap();

        let mut session_detail = detail("raw-current");
        session_detail.normalized_session_id = Some("main-session".to_string());
        session_detail.session_scope = None;

        let action = pending_action_for_session(&state, &session_detail).unwrap();

        assert!(action.is_none());
    }

    fn test_state(name: &str) -> AppState {
        let store = NiumaStore::new(std::env::temp_dir().join(format!(
            "niuma_api_{name}_{}.sqlite",
            Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )));
        let runtime_events = RuntimeEventBus::new();
        let mutation_service = StateMutationService::new(store.clone(), runtime_events.clone());
        AppState {
            store,
            runtime_events,
            mutation_service,
            approval_arbiter: Arc::new(Mutex::new(ApprovalArbiter::default())),
            plugin_dir: std::env::temp_dir(),
            main_state_broadcaster: Arc::new(Mutex::new(MainStateBroadcaster::default())),
            tool_sessions: ToolSessionRegistry::new(),
        }
    }

    fn detail(session_id: &str) -> ToolSessionDetail {
        ToolSessionDetail {
            tool: ToolKind::Codex,
            session_id: session_id.to_string(),
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            is_subagent: false,
            parent_session_id: None,
            normalized_session_id: Some(session_id.to_string()),
            session_scope: Some(ToolSessionScope::Main),
            agent_nickname: None,
            agent_role: None,
            normalization_status: None,
            control: None,
            pending_action: None,
            messages: Vec::new(),
            next_cursor: None,
        }
    }

    fn approval_event(id: &str, session_id: &str, timestamp: i64) -> NiumaEvent {
        let mut interaction = EventInteractionDetail::niuma_approval("approval-request-1");
        interaction.message = Some("请批准 cargo test".to_string());
        sample_event(
            id,
            session_id,
            EventType::ApprovalRequested,
            timestamp,
            "Bash: cargo test",
            Some(interaction),
        )
    }

    fn input_event(id: &str, session_id: &str, timestamp: i64) -> NiumaEvent {
        let mut interaction = EventInteractionDetail {
            kind: EventInteractionKind::Input,
            handling: EventInteractionHandling::Niuma,
            actionable: true,
            request_id: Some("input-request-1".to_string()),
            control_ref: Some(ApprovalControlRef {
                channel_id: "channel-1".to_string(),
                codex_session_id: None,
                relay_request_id: "relay-1".to_string(),
                turn_id: None,
                item_id: None,
            }),
            actions: vec!["submit".to_string()],
            endpoint: Some("/api/v1/session-control/answer-input".to_string()),
            message: Some("请选择运行方式".to_string()),
            schema: Some(EventInteractionSchema {
                questions: vec![
                    EventInteractionQuestion {
                        id: "mode".to_string(),
                        header: Some("运行形态".to_string()),
                        question: "你希望主要以什么形态运行？".to_string(),
                        options: vec![EventInteractionOption {
                            label: "托盘常驻".to_string(),
                            description: Some("适合长期后台监控".to_string()),
                        }],
                    },
                    EventInteractionQuestion {
                        id: "notes".to_string(),
                        header: None,
                        question: "补充说明".to_string(),
                        options: Vec::new(),
                    },
                ],
            }),
        };
        interaction.control_ref.as_mut().unwrap().turn_id = Some("turn-1".to_string());
        sample_event(
            id,
            session_id,
            EventType::InputRequested,
            timestamp,
            "等待输入",
            Some(interaction),
        )
    }

    fn sample_event(
        id: &str,
        session_id: &str,
        event_type: EventType,
        timestamp: i64,
        summary: &str,
        interaction: Option<EventInteractionDetail>,
    ) -> NiumaEvent {
        NiumaEvent {
            id: id.to_string(),
            dedupe_key: format!("dedupe-{id}"),
            source: "test".to_string(),
            tool: ToolKind::Codex,
            session_id: session_id.to_string(),
            parent_session_id: None,
            normalized_session_id: None,
            session_scope: None,
            agent_nickname: None,
            agent_role: None,
            tool_call_id: None,
            project_path: "/tmp/demo".to_string(),
            project_name: "demo".to_string(),
            event_type,
            severity: "info".to_string(),
            summary: summary.to_string(),
            content: None,
            error_message: None,
            attention_resolve_key: None,
            completion_reason: None,
            failure_reason: None,
            payload_ref: None,
            interaction,
            created_at: Utc.timestamp_opt(timestamp, 0).single().unwrap(),
        }
    }
}
