use crate::models::{
    AttentionItem, EventType, LatestActivity, NiumaEvent, NiumaSession, SessionStatus,
};
use crate::store::StoredState;

pub(super) fn already_applied(state: &StoredState, event: &NiumaEvent) -> bool {
    state
        .sessions
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

pub(super) fn is_late_terminal_activity(sessions: &[NiumaSession], event: &NiumaEvent) -> bool {
    if event.event_type != EventType::SessionActivity {
        return false;
    }
    sessions
        .iter()
        .find(|session| session.id == event.session_id)
        .map(|session| {
            // Codex 可能在终止事件后继续写 token_count 等遥测行，不能用这些行重新打开任务。
            matches!(
                session.status,
                SessionStatus::Completed
                    | SessionStatus::Error
                    | SessionStatus::Stale
                    | SessionStatus::Idle
            )
        })
        .unwrap_or(false)
}

pub(super) fn upsert_session(sessions: &mut Vec<NiumaSession>, event: &NiumaEvent) {
    let status = status_from_event(&event.event_type);
    if let Some(session) = sessions
        .iter_mut()
        .find(|session| session.id == event.session_id)
    {
        session.status = status;
        if !event.project_path.trim().is_empty() {
            session.project_path = event.project_path.clone();
            session.project_name = event.project_name.clone();
        }
        session.tool = event.tool.clone();
        session.last_event_id = Some(event.id.clone());
        session.last_activity_at = event.created_at;
        return;
    }

    sessions.push(NiumaSession {
        id: event.session_id.clone(),
        tool: event.tool.clone(),
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

    let status = status_from_event(&event.event_type);
    match status {
        SessionStatus::WaitingApproval | SessionStatus::WaitingInput | SessionStatus::Error => {
            state
                .attention_items
                .push(AttentionItem::from_event(event, status));
        }
        SessionStatus::Running | SessionStatus::Completed => {
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
        SessionStatus::Stale => {
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
        SessionStatus::Idle => {
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
        && item.status == SessionStatus::WaitingApproval
        && item.attention_resolve_key.is_none()
}

fn is_keyed_approval(item: &AttentionItem) -> bool {
    item.status == SessionStatus::WaitingApproval && item.attention_resolve_key.is_some()
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
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
    {
        // session 列表应反映仍未解决的阻塞项，避免最新普通活动把等待批准显示成运行中。
        session.status = attention_status;
    }
}

fn status_from_event(event_type: &EventType) -> SessionStatus {
    match event_type {
        EventType::SessionStarted => SessionStatus::Running,
        EventType::SessionIdled => SessionStatus::Idle,
        EventType::ApprovalRequested => SessionStatus::WaitingApproval,
        EventType::InputRequested => SessionStatus::WaitingInput,
        EventType::TaskFailed => SessionStatus::Error,
        EventType::AssistantMessageCompleted => SessionStatus::Completed,
        EventType::ManualDismissed => SessionStatus::Completed,
        EventType::SessionStaled => SessionStatus::Stale,
        EventType::SessionActivity => SessionStatus::Running,
    }
}
