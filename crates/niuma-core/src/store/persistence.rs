use rusqlite::{params, Connection};
use serde::Serialize;

use crate::store::StoredState;

pub(super) fn save_state(connection: &Connection, state: &StoredState) -> Result<(), String> {
    save_small_state_tables(connection, state)
}

fn save_small_state_tables(connection: &Connection, state: &StoredState) -> Result<(), String> {
    connection
        .execute_batch(
            "
            DELETE FROM sessions;
            DELETE FROM attention_items;
            DELETE FROM latest_activity;
            ",
        )
        .map_err(|error| format!("清空 SQLite 小状态表失败：{error}"))?;

    for session in &state.sessions {
        connection
            .execute(
                "INSERT INTO sessions
                 (id, tool, project_path, project_name, status, last_event_id, last_activity_at, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    session.id,
                    stable_text(&session.tool, "session tool")?,
                    session.project_path,
                    session.project_name,
                    stable_text(&session.status, "session status")?,
                    session.last_event_id,
                    session.last_activity_at.to_rfc3339(),
                    serde_json::to_string(session)
                        .map_err(|error| format!("序列化 session 失败：{error}"))?
                ],
            )
            .map_err(|error| format!("写入 session 失败：{error}"))?;
    }

    for item in &state.attention_items {
        connection
            .execute(
                "INSERT INTO attention_items
                 (event_id, session_id, status, attention_resolve_key, created_at, payload)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    item.event_id,
                    item.session_id,
                    stable_text(&item.status, "attention status")?,
                    item.attention_resolve_key,
                    item.created_at.to_rfc3339(),
                    serde_json::to_string(item)
                        .map_err(|error| format!("序列化待处理项失败：{error}"))?
                ],
            )
            .map_err(|error| format!("写入待处理项失败：{error}"))?;
    }

    if let Some(activity) = &state.latest_activity {
        connection
            .execute(
                "INSERT INTO latest_activity
                 (id, event_id, session_id, status, updated_at, payload)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)",
                params![
                    activity.event_id,
                    activity.session_id,
                    stable_text(&activity.status, "latest activity status")?,
                    activity.updated_at.map(|value| value.to_rfc3339()),
                    serde_json::to_string(activity)
                        .map_err(|error| format!("序列化 latest_activity 失败：{error}"))?
                ],
            )
            .map_err(|error| format!("写入 latest_activity 失败：{error}"))?;
    }

    Ok(())
}

pub(super) fn load_json_rows<T: serde::de::DeserializeOwned>(
    connection: &Connection,
    table: &str,
    order_by: &str,
) -> Result<Vec<T>, String> {
    let sql = format!("SELECT payload FROM {table} ORDER BY {order_by}");
    let mut statement = connection
        .prepare(&sql)
        .map_err(|error| format!("读取 {table} 失败：{error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("读取 {table} 失败：{error}"))?;

    let mut values = Vec::new();
    for row in rows {
        let payload = row.map_err(|error| format!("读取 {table} 行失败：{error}"))?;
        values.push(
            serde_json::from_str(&payload)
                .map_err(|error| format!("解析 {table} 行失败：{error}"))?,
        );
    }
    Ok(values)
}

fn stable_text(value: &impl Serialize, label: &str) -> Result<String, String> {
    let value =
        serde_json::to_value(value).map_err(|error| format!("序列化 {label} 失败：{error}"))?;
    value
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("{label} 必须序列化为字符串"))
}
