use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;

use crate::models::{EventType, NiumaEvent};

pub(super) fn clear_public_events(connection: &Connection) -> Result<(), String> {
    connection
        .execute("DELETE FROM public_events", [])
        .map_err(|error| format!("清空公开事件失败：{error}"))?;
    Ok(())
}

pub(super) fn append_public_event(
    connection: &Connection,
    event: &NiumaEvent,
) -> Result<(), String> {
    if !is_public_event(event) {
        return Ok(());
    }
    connection
        .execute(
            "INSERT OR REPLACE INTO public_events
             (id, dedupe_key, source, tool, session_id, project_path, project_name, event_type, severity, created_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            params![
                event.id,
                event.dedupe_key,
                event.source,
                stable_text(&event.tool, "event tool")?,
                event.session_id,
                event.project_path,
                event.project_name,
                stable_text(&event.event_type, "event type")?,
                event.severity,
                event.created_at.to_rfc3339(),
                serde_json::to_string(event)
                    .map_err(|error| format!("序列化公开事件失败：{error}"))?
            ],
        )
        .map_err(|error| format!("写入公开事件失败：{error}"))?;
    Ok(())
}

pub(super) fn load_public_events(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<NiumaEvent>, String> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut statement = connection
        .prepare("SELECT payload FROM public_events ORDER BY created_at DESC, id DESC LIMIT ?1")
        .map_err(|error| format!("读取公开事件失败：{error}"))?;
    let rows = statement
        .query_map([limit as i64], |row| row.get::<_, String>(0))
        .map_err(|error| format!("读取公开事件失败：{error}"))?;

    let mut events = Vec::new();
    for row in rows {
        let payload = row.map_err(|error| format!("读取公开事件行失败：{error}"))?;
        events.push(
            serde_json::from_str(&payload).map_err(|error| format!("解析公开事件失败：{error}"))?,
        );
    }
    Ok(events)
}

pub(super) fn load_public_event_by_id(
    connection: &Connection,
    event_id: &str,
) -> Result<Option<NiumaEvent>, String> {
    let payload = connection
        .query_row(
            "SELECT payload FROM public_events WHERE id = ?1",
            [event_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("读取公开事件失败：{error}"))?;
    payload
        .map(|payload| {
            serde_json::from_str(&payload).map_err(|error| format!("解析公开事件失败：{error}"))
        })
        .transpose()
}

fn is_public_event(event: &NiumaEvent) -> bool {
    !matches!(
        event.event_type,
        EventType::SessionActivity | EventType::SessionStaled
    )
}

fn stable_text(value: &impl Serialize, label: &str) -> Result<String, String> {
    let value =
        serde_json::to_value(value).map_err(|error| format!("序列化 {label} 失败：{error}"))?;
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{label} 必须序列化为字符串"))
}
