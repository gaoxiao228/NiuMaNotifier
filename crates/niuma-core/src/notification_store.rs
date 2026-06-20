use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Error as SqliteError, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::io;

use crate::models::EventType;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    Bark,
    Ntfy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationRecordStatus {
    Pending,
    Sent,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationNotifierType {
    Builtin,
    Plugin,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationRecord {
    pub id: String,
    pub notifier_id: String,
    pub notifier_type: NotificationNotifierType,
    pub event_id: String,
    pub event_type: EventType,
    pub status: NotificationRecordStatus,
    pub title: Option<String>,
    pub body: Option<String>,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PluginNotificationResult {
    pub id: String,
    pub plugin_id: String,
    pub event_id: String,
    pub event_type: EventType,
    pub status: NotificationRecordStatus,
    pub title: Option<String>,
    pub body: Option<String>,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationHistoryRecord {
    pub id: String,
    pub event_id: String,
    pub event_type: EventType,
    pub channel: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    pub status: NotificationRecordStatus,
    pub title: Option<String>,
    pub body: Option<String>,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}

pub(crate) fn insert_record_if_absent(
    connection: &Connection,
    record: &NotificationRecord,
) -> Result<bool, String> {
    let changed = connection
        .execute(
            "INSERT INTO notification_records
             (id, notifier_id, notifier_type, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(notifier_id, event_id) DO NOTHING",
            params![
                &record.id,
                &record.notifier_id,
                notifier_type_id(&record.notifier_type),
                &record.event_id,
                serde_json::to_string(&record.event_type)
                    .map_err(|error| format!("序列化通知事件类型失败：{error}"))?,
                serde_json::to_string(&record.status)
                    .map_err(|error| format!("序列化通知记录状态失败：{error}"))?,
                &record.title,
                &record.body,
                &record.reason,
                &record.error_message,
                record.created_at.to_rfc3339(),
                record.sent_at.map(|value| value.to_rfc3339()),
            ],
        )
        .map_err(|error| format!("写入通知记录失败：{error}"))?;
    Ok(changed == 1)
}

pub(crate) fn update_record_result(
    connection: &Connection,
    record_id: &str,
    status: &NotificationRecordStatus,
    error_message: Option<&str>,
    sent_at: Option<DateTime<Utc>>,
) -> Result<(), String> {
    let changed = connection
        .execute(
            "UPDATE notification_records
             SET status = ?2, error_message = ?3, sent_at = ?4
             WHERE id = ?1",
            params![
                record_id,
                serde_json::to_string(status)
                    .map_err(|error| format!("序列化通知记录状态失败：{error}"))?,
                error_message,
                sent_at.map(|value| value.to_rfc3339()),
            ],
        )
        .map_err(|error| format!("更新通知记录失败：{error}"))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(format!("通知记录不存在：{record_id}"))
    }
}

pub(crate) fn upsert_plugin_result(
    connection: &Connection,
    result: &PluginNotificationResult,
) -> Result<(), String> {
    let record = NotificationRecord::from(result.clone());
    connection
        .execute(
            "INSERT INTO notification_records
             (id, notifier_id, notifier_type, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(notifier_id, event_id) DO UPDATE SET
               status = excluded.status,
               title = excluded.title,
               body = excluded.body,
               reason = excluded.reason,
               error_message = excluded.error_message,
               created_at = excluded.created_at,
               sent_at = excluded.sent_at",
            params![
                &record.id,
                &record.notifier_id,
                notifier_type_id(&record.notifier_type),
                &record.event_id,
                serde_json::to_string(&record.event_type)
                    .map_err(|error| format!("序列化插件通知事件类型失败：{error}"))?,
                serde_json::to_string(&record.status)
                    .map_err(|error| format!("序列化插件通知状态失败：{error}"))?,
                &record.title,
                &record.body,
                &record.reason,
                &record.error_message,
                record.created_at.to_rfc3339(),
                record.sent_at.map(|value| value.to_rfc3339()),
            ],
        )
        .map_err(|error| format!("保存插件通知结果失败：{error}"))?;
    Ok(())
}

pub(crate) fn load_plugin_result(
    connection: &Connection,
    plugin_id: &str,
    event_id: &str,
) -> Result<Option<PluginNotificationResult>, String> {
    let mut statement = connection
        .prepare(
            "SELECT id, notifier_id, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at
             FROM notification_records
             WHERE notifier_type = 'plugin' AND notifier_id = ?1 AND event_id = ?2",
        )
        .map_err(|error| format!("读取插件通知结果失败：{error}"))?;
    statement
        .query_row(params![plugin_id, event_id], plugin_result_from_row)
        .optional()
        .map_err(|error| format!("读取插件通知结果行失败：{error}"))
}

pub(crate) fn load_records(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<NotificationRecord>, String> {
    let limit = i64::try_from(limit).unwrap_or(20).clamp(1, 200);
    let mut statement = connection
        .prepare(
            "SELECT id, notifier_id, notifier_type, event_id, event_type, status, title, body, reason, error_message, created_at, sent_at
             FROM notification_records
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .map_err(|error| format!("读取通知记录失败：{error}"))?;
    let rows = statement
        .query_map([limit], |row| {
            let notifier_type_text: String = row.get(2)?;
            let event_type_text: String = row.get(4)?;
            let status_text: String = row.get(5)?;
            let created_at_text: String = row.get(10)?;
            let sent_at_text: Option<String> = row.get(11)?;
            let notifier_type = parse_notifier_type(&notifier_type_text).map_err(|error| {
                from_parse_error(2, io::Error::new(io::ErrorKind::InvalidData, error))
            })?;
            let event_type = serde_json::from_str(&event_type_text)
                .map_err(|error| from_parse_error(4, error))?;
            let status =
                serde_json::from_str(&status_text).map_err(|error| from_parse_error(5, error))?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_text)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| from_parse_error(10, error))?;
            let sent_at = match sent_at_text {
                Some(text) => Some(
                    DateTime::parse_from_rfc3339(&text)
                        .map(|value| value.with_timezone(&Utc))
                        .map_err(|error| from_parse_error(11, error))?,
                ),
                None => None,
            };
            Ok(NotificationRecord {
                id: row.get(0)?,
                notifier_id: row.get(1)?,
                notifier_type,
                event_id: row.get(3)?,
                event_type,
                status,
                title: row.get(6)?,
                body: row.get(7)?,
                reason: row.get(8)?,
                error_message: row.get(9)?,
                created_at,
                sent_at,
            })
        })
        .map_err(|error| format!("读取通知记录失败：{error}"))?;

    let mut records = Vec::new();
    for row in rows {
        records.push(row.map_err(|error| format!("读取通知记录行失败：{error}"))?);
    }
    Ok(records)
}

pub(crate) fn load_history_records(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<NotificationHistoryRecord>, String> {
    Ok(load_records(connection, limit)?
        .into_iter()
        .map(NotificationHistoryRecord::from)
        .collect())
}

fn plugin_result_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<PluginNotificationResult> {
    let event_type_text: String = row.get(3)?;
    let status_text: String = row.get(4)?;
    let created_at_text: String = row.get(9)?;
    let sent_at_text: Option<String> = row.get(10)?;
    let event_type =
        serde_json::from_str(&event_type_text).map_err(|error| from_parse_error(3, error))?;
    let status = serde_json::from_str(&status_text).map_err(|error| from_parse_error(4, error))?;
    let created_at = DateTime::parse_from_rfc3339(&created_at_text)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| from_parse_error(9, error))?;
    let sent_at = match sent_at_text {
        Some(text) => Some(
            DateTime::parse_from_rfc3339(&text)
                .map(|value| value.with_timezone(&Utc))
                .map_err(|error| from_parse_error(10, error))?,
        ),
        None => None,
    };
    Ok(PluginNotificationResult {
        id: row.get(0)?,
        plugin_id: row.get(1)?,
        event_id: row.get(2)?,
        event_type,
        status,
        title: row.get(5)?,
        body: row.get(6)?,
        reason: row.get(7)?,
        error_message: row.get(8)?,
        created_at,
        sent_at,
    })
}

impl From<NotificationRecord> for NotificationHistoryRecord {
    fn from(record: NotificationRecord) -> Self {
        let plugin_id = match record.notifier_type {
            NotificationNotifierType::Builtin => None,
            NotificationNotifierType::Plugin => Some(record.notifier_id.clone()),
        };
        Self {
            id: record.id,
            event_id: record.event_id,
            event_type: record.event_type,
            channel: record.notifier_id,
            plugin_id,
            status: record.status,
            title: record.title,
            body: record.body,
            reason: record.reason,
            error_message: record.error_message,
            created_at: record.created_at,
            sent_at: record.sent_at,
        }
    }
}

impl From<PluginNotificationResult> for NotificationRecord {
    fn from(record: PluginNotificationResult) -> Self {
        Self {
            id: record.id,
            notifier_id: record.plugin_id,
            notifier_type: NotificationNotifierType::Plugin,
            event_id: record.event_id,
            event_type: record.event_type,
            status: record.status,
            title: record.title,
            body: record.body,
            reason: record.reason,
            error_message: record.error_message,
            created_at: record.created_at,
            sent_at: record.sent_at,
        }
    }
}

impl From<PluginNotificationResult> for NotificationHistoryRecord {
    fn from(record: PluginNotificationResult) -> Self {
        Self {
            id: record.id,
            event_id: record.event_id,
            event_type: record.event_type,
            channel: record.plugin_id.clone(),
            plugin_id: Some(record.plugin_id),
            status: record.status,
            title: record.title,
            body: record.body,
            reason: record.reason,
            error_message: record.error_message,
            created_at: record.created_at,
            sent_at: record.sent_at,
        }
    }
}

// SQLite 中使用稳定字符串作为唯一键，避免 enum 序列化格式变更影响去重。
pub fn channel_id(channel: &NotificationChannel) -> &'static str {
    match channel {
        NotificationChannel::Bark => "bark",
        NotificationChannel::Ntfy => "ntfy",
    }
}

fn notifier_type_id(value: &NotificationNotifierType) -> &'static str {
    match value {
        NotificationNotifierType::Builtin => "builtin",
        NotificationNotifierType::Plugin => "plugin",
    }
}

fn parse_notifier_type(value: &str) -> Result<NotificationNotifierType, String> {
    match value {
        "builtin" => Ok(NotificationNotifierType::Builtin),
        "plugin" => Ok(NotificationNotifierType::Plugin),
        _ => Err(format!("未知通知器类型：{value}")),
    }
}

pub fn channel_from_id(value: &str) -> Result<NotificationChannel, String> {
    parse_channel_id(value)
}

fn parse_channel_id(value: &str) -> Result<NotificationChannel, String> {
    match value {
        "bark" => Ok(NotificationChannel::Bark),
        "ntfy" => Ok(NotificationChannel::Ntfy),
        _ => Err(format!("未知通知渠道：{value}")),
    }
}

fn from_parse_error(
    column: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> SqliteError {
    SqliteError::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(error))
}
