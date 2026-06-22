use crate::models::{
    AttentionItem, EventType, LatestActivity, NiumaEvent, RuntimeStateItem, RuntimeStateStatus,
};
use crate::store::StoredState;

pub(super) fn already_applied(state: &StoredState, event: &NiumaEvent) -> bool {
    state
        .runtime_states
        .iter()
        .any(|session| session.last_event_id.as_deref() == Some(event.id.as_str()))
        || state
            .attention_items
            .iter()
            .any(|item| item.event_id == event.id)
        || state
            .latest_activity
            .as_ref()
            .and_then(|activity| activity.event_id.as_deref())
            == Some(event.id.as_str())
}

pub(super) fn is_late_terminal_activity(
    runtime_states: &[RuntimeStateItem],
    event: &NiumaEvent,
) -> bool {
    if event.event_type != EventType::SessionActivity {
        return false;
    }
    runtime_states
        .iter()
        .find(|item| item.tool == event.tool && item.session_id == event.session_id)
        .map(|item| {
            // Codex 可能在终止事件后继续写 token_count 等遥测行，不能用这些行重新打开任务。
            matches!(
                item.status,
                RuntimeStateStatus::Completed
                    | RuntimeStateStatus::Error
                    | RuntimeStateStatus::Stale
                    | RuntimeStateStatus::Idle
            )
        })
        .unwrap_or(false)
}

pub(super) fn upsert_runtime_state(runtime_states: &mut Vec<RuntimeStateItem>, event: &NiumaEvent) {
    let status = status_from_event(&event.event_type);
    if let Some(item) = runtime_states
        .iter_mut()
        .find(|item| item.tool == event.tool && item.session_id == event.session_id)
    {
        item.status = status;
        if !event.project_path.trim().is_empty() {
            item.project_path = event.project_path.clone();
            item.project_name = event.project_name.clone();
        }
        item.tool = event.tool.clone();
        item.last_event_id = Some(event.id.clone());
        item.last_activity_at = event.created_at;
        return;
    }

    runtime_states.push(RuntimeStateItem {
        tool: event.tool.clone(),
        session_id: event.session_id.clone(),
        project_path: event.project_path.clone(),
        project_name: event.project_name.clone(),
        status,
        last_event_id: Some(event.id.clone()),
        last_activity_at: event.created_at,
    });
}

pub(super) fn apply_attention_transition(state: &mut StoredState, event: &NiumaEvent) {
    if matches!(event.event_type, EventType::ManualDismissed) {
        state.attention_items.clear();
        return;
    }
    if matches!(event.event_type, EventType::ApprovalReturnedToCodex) {
        restore_session_attention_status(state, &event.session_id);
        return;
    }

    let status = status_from_event(&event.event_type);
    match status {
        RuntimeStateStatus::WaitingApproval
        | RuntimeStateStatus::WaitingInput
        | RuntimeStateStatus::Error => {
            state
                .attention_items
                .push(AttentionItem::from_event(event, status));
        }
        RuntimeStateStatus::Running | RuntimeStateStatus::Completed => {
            if let Some(resolve_key) = event.attention_resolve_key.as_deref() {
                // 授权恢复事件只移除对应审批；保留同 session 的等待输入和错误。
                state.attention_items.retain(|item| {
                    item.attention_resolve_key.as_deref() != Some(resolve_key)
                        && !is_unkeyed_approval_for_session(item, &event.session_id)
                });
            } else {
                state
                    .attention_items
                    .retain(|item| item.session_id != event.session_id || is_keyed_approval(item));
            }
            restore_session_attention_status(state, &event.session_id);
            state.latest_activity = Some(LatestActivity::from_event(event, status));
        }
        RuntimeStateStatus::Stale => {
            // stale 是内部清理态：移除当前 session 的残留关注项，并只在命中当前活动时回到 idle。
            state
                .attention_items
                .retain(|item| item.session_id != event.session_id);
            if state
                .latest_activity
                .as_ref()
                .and_then(|activity| activity.session_id.as_deref())
                == Some(event.session_id.as_str())
            {
                state.latest_activity = Some(LatestActivity::idle());
            }
        }
        RuntimeStateStatus::Idle => {
            // 手动测试的 idle 表示当前 session 已无活动，需要清掉它自己的阻塞项。
            state
                .attention_items
                .retain(|item| item.session_id != event.session_id);
            state.latest_activity = Some(LatestActivity::idle());
        }
    }
}

fn is_unkeyed_approval_for_session(item: &AttentionItem, session_id: &str) -> bool {
    item.session_id == session_id
        && item.status == RuntimeStateStatus::WaitingApproval
        && item.attention_resolve_key.is_none()
}

fn is_keyed_approval(item: &AttentionItem) -> bool {
    item.status == RuntimeStateStatus::WaitingApproval && item.attention_resolve_key.is_some()
}

fn restore_session_attention_status(state: &mut StoredState, session_id: &str) {
    let Some(attention_status) = state
        .attention_items
        .iter()
        .find(|item| item.session_id == session_id)
        .map(|item| item.status.clone())
    else {
        return;
    };
    if let Some(session) = state
        .runtime_states
        .iter_mut()
        .find(|session| session.session_id == session_id)
    {
        // 运行态列表应反映仍未解决的阻塞项，避免最新普通活动把等待批准显示成运行中。
        session.status = attention_status;
    }
}

fn status_from_event(event_type: &EventType) -> RuntimeStateStatus {
    match event_type {
        EventType::SessionStarted => RuntimeStateStatus::Running,
        EventType::SessionIdled => RuntimeStateStatus::Idle,
        EventType::ApprovalRequested => RuntimeStateStatus::WaitingApproval,
        EventType::ApprovalReturnedToCodex => RuntimeStateStatus::WaitingApproval,
        EventType::ApprovalResolved => RuntimeStateStatus::Running,
        EventType::InputRequested => RuntimeStateStatus::WaitingInput,
        EventType::TaskFailed => RuntimeStateStatus::Error,
        EventType::AssistantMessageCompleted => RuntimeStateStatus::Completed,
        EventType::ManualDismissed => RuntimeStateStatus::Completed,
        EventType::SessionStaled => RuntimeStateStatus::Stale,
        EventType::SessionActivity => RuntimeStateStatus::Running,
    }
}
