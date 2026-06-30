use crate::models::{
    AttentionItem, EventSessionScope, EventType, LatestActivity, NiumaEvent, RuntimeStateItem,
    RuntimeStateStatus, ToolKind,
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
    if matches!(
        event.event_type,
        EventType::ApprovalReturnedToCodex | EventType::ApprovalReturnedToTool
    ) {
        restore_session_attention_status(state, &event.tool, &event.session_id);
        return;
    }

    let status = status_from_event(&event.event_type);
    match status {
        RuntimeStateStatus::WaitingApproval
        | RuntimeStateStatus::WaitingInput
        | RuntimeStateStatus::Error => {
            if should_skip_unkeyed_input_fallback(state, event) {
                return;
            }
            if is_keyed_relay_input_event(event) {
                // relay input 带有可操作 request_id，应覆盖同一运行态的 watcher fallback。
                state.attention_items.retain(|item| {
                    !is_unkeyed_input_for_session(item, &event.tool, &event.session_id)
                });
            }
            state
                .attention_items
                .push(AttentionItem::from_event(event, status));
        }
        RuntimeStateStatus::Running | RuntimeStateStatus::Completed => {
            if let Some(resolve_key) = event.attention_resolve_key.as_deref() {
                // 授权恢复事件只移除同一运行态身份下的对应审批，保留其他工具的同名 session。
                state.attention_items.retain(|item| {
                    !attention_item_matches_event(item, event)
                        || !attention_item_resolved_by_key(item, event, resolve_key)
                });
            } else {
                state.attention_items.retain(|item| {
                    !attention_item_matches_event(item, event) || is_keyed_approval(item)
                });
            }
            restore_session_attention_status(state, &event.tool, &event.session_id);
            if should_update_latest_activity(event) {
                state.latest_activity = Some(LatestActivity::from_event(event, status));
            }
        }
        RuntimeStateStatus::Stale => {
            // stale 是内部清理态：移除当前运行态身份的残留关注项，并只在命中当前活动时回到 idle。
            state
                .attention_items
                .retain(|item| !attention_item_matches_event(item, event));
            if state
                .latest_activity
                .as_ref()
                .map(|activity| latest_activity_matches_event(activity, event))
                .unwrap_or(false)
            {
                state.latest_activity = Some(LatestActivity::idle());
            }
        }
        RuntimeStateStatus::Idle => {
            // 手动测试的 idle 表示当前运行态身份已无活动，需要清掉它自己的阻塞项。
            state
                .attention_items
                .retain(|item| !attention_item_matches_event(item, event));
            if state
                .latest_activity
                .as_ref()
                .map(|activity| latest_activity_matches_event(activity, event))
                .unwrap_or(false)
            {
                // idle 只能清理同一 tool + session_id 的最新活动，避免同名 session 跨工具串扰。
                state.latest_activity = Some(LatestActivity::idle());
            }
        }
    }
}

fn should_update_latest_activity(event: &NiumaEvent) -> bool {
    // subagent 完成只表示子任务结束，不能把主状态顶成 completed；事件仍保留在事件中心/SSE。
    !(event.event_type == EventType::AssistantMessageCompleted
        && event.session_scope == Some(EventSessionScope::Subagent))
}

fn attention_item_matches_event(item: &AttentionItem, event: &NiumaEvent) -> bool {
    item.tool == event.tool && item.session_id == event.session_id
}

fn latest_activity_matches_event(activity: &LatestActivity, event: &NiumaEvent) -> bool {
    activity.tool.as_ref() == Some(&event.tool)
        && activity.session_id.as_deref() == Some(event.session_id.as_str())
}

fn is_unkeyed_approval_for_session(
    item: &AttentionItem,
    tool: &ToolKind,
    session_id: &str,
) -> bool {
    &item.tool == tool
        && item.session_id == session_id
        && item.status == RuntimeStateStatus::WaitingApproval
        && item.attention_resolve_key.is_none()
}

fn is_unkeyed_input_for_session(item: &AttentionItem, tool: &ToolKind, session_id: &str) -> bool {
    &item.tool == tool
        && item.session_id == session_id
        && item.status == RuntimeStateStatus::WaitingInput
        && item.attention_resolve_key.is_none()
}

fn is_keyed_approval(item: &AttentionItem) -> bool {
    item.status == RuntimeStateStatus::WaitingApproval && item.attention_resolve_key.is_some()
}

fn is_keyed_input(item: &AttentionItem) -> bool {
    item.status == RuntimeStateStatus::WaitingInput
        && item
            .attention_resolve_key
            .as_deref()
            .map(is_input_resolve_key)
            .unwrap_or(false)
}

fn is_keyed_relay_input_event(event: &NiumaEvent) -> bool {
    event.event_type == EventType::InputRequested
        && event
            .attention_resolve_key
            .as_deref()
            .map(is_input_resolve_key)
            .unwrap_or(false)
}

fn should_skip_unkeyed_input_fallback(state: &StoredState, event: &NiumaEvent) -> bool {
    event.event_type == EventType::InputRequested
        && event.attention_resolve_key.is_none()
        && state
            .attention_items
            .iter()
            .any(|item| attention_item_matches_event(item, event) && is_keyed_input(item))
}

fn attention_item_resolved_by_key(
    item: &AttentionItem,
    event: &NiumaEvent,
    resolve_key: &str,
) -> bool {
    item.attention_resolve_key.as_deref() == Some(resolve_key)
        || (is_approval_resolve_key(resolve_key)
            && is_unkeyed_approval_for_session(item, &event.tool, &event.session_id))
        || (is_input_resolve_key(resolve_key)
            && is_unkeyed_input_for_session(item, &event.tool, &event.session_id))
}

fn is_input_resolve_key(resolve_key: &str) -> bool {
    resolve_key.starts_with("input:")
}

fn is_approval_resolve_key(resolve_key: &str) -> bool {
    resolve_key.starts_with("approval:")
        || resolve_key.starts_with("codex_permission:")
        || resolve_key.starts_with("claude_permission:")
}

fn restore_session_attention_status(state: &mut StoredState, tool: &ToolKind, session_id: &str) {
    let Some(attention_status) = state
        .attention_items
        .iter()
        .find(|item| &item.tool == tool && item.session_id == session_id)
        .map(|item| item.status.clone())
    else {
        return;
    };
    if let Some(session) = state
        .runtime_states
        .iter_mut()
        .find(|session| &session.tool == tool && session.session_id == session_id)
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
        EventType::ApprovalReturnedToCodex | EventType::ApprovalReturnedToTool => {
            RuntimeStateStatus::WaitingApproval
        }
        EventType::ApprovalResolved => RuntimeStateStatus::Running,
        EventType::InputRequested => RuntimeStateStatus::WaitingInput,
        EventType::TaskFailed => RuntimeStateStatus::Error,
        EventType::AssistantMessageCompleted => RuntimeStateStatus::Completed,
        EventType::ManualDismissed => RuntimeStateStatus::Completed,
        EventType::SessionStaled => RuntimeStateStatus::Stale,
        EventType::SessionActivity => RuntimeStateStatus::Running,
    }
}
