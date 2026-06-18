use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, Error as SqliteError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;

use crate::models::EventType;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotificationChannel {
    Bark,
    Ntfy,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct NotificationChannelConfig {
    pub channel: NotificationChannel,
    pub enabled: bool,
    pub payload: Value,
    pub updated_at: DateTime<Utc>,
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
pub struct NotificationRecord {
    pub id: String,
    pub event_id: String,
    pub event_type: EventType,
    pub channel: NotificationChannel,
    pub status: NotificationRecordStatus,
    pub title: Option<String>,
    pub body: Option<String>,
    pub reason: Option<String>,
    pub error_message: Option<String>,
    pub created_at: DateTime<Utc>,
    pub sent_at: Option<DateTime<Utc>>,
}

pub(crate) fn load_channels(
    connection: &Connection,
) -> Result<Vec<NotificationChannelConfig>, String> {
    let mut statement = connection
        .prepare("SELECT payload FROM notification_channels ORDER BY id ASC")
        .map_err(|error| format!("读取通知渠道配置失败：{error}"))?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|error| format!("读取通知渠道配置失败：{error}"))?;

    let mut values = Vec::new();
    for row in rows {
        let payload = row.map_err(|error| format!("读取通知渠道配置行失败：{error}"))?;
        values.push(
            serde_json::from_str::<NotificationChannelConfig>(&payload)
                .map_err(|error| format!("解析通知渠道配置失败：{error}"))?,
        );
    }
    Ok(values)
}

pub(crate) fn save_channels(
    connection: &Connection,
    configs: &[NotificationChannelConfig],
) -> Result<(), String> {
    for config in configs {
        let id = channel_id(&config.channel);
        connection
            .execute(
                "INSERT INTO notification_channels (id, channel, enabled, payload, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                   channel = excluded.channel,
                   enabled = excluded.enabled,
                   payload = excluded.payload,
                   updated_at = excluded.updated_at",
                params![
                    id,
                    id,
                    if config.enabled { 1_i64 } else { 0_i64 },
                    serde_json::to_string(config)
                        .map_err(|error| format!("序列化通知渠道配置失败：{error}"))?,
                    config.updated_at.to_rfc3339(),
                ],
            )
            .map_err(|error| format!("保存通知渠道配置失败：{error}"))?;
    }
    Ok(())
}

pub(crate) fn insert_record_if_absent(
    connection: &Connection,
    record: &NotificationRecord,
) -> Result<bool, String> {
    let changed = connection
        .execute(
            "INSERT INTO notification_records
             (id, event_id, event_type, channel, status, title, body, reason, error_message, created_at, sent_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(event_id, channel) DO NOTHING",
            params![
                &record.id,
                &record.event_id,
                serde_json::to_string(&record.event_type)
                    .map_err(|error| format!("序列化通知事件类型失败：{error}"))?,
                channel_id(&record.channel),
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

pub(crate) fn load_records(
    connection: &Connection,
    limit: usize,
) -> Result<Vec<NotificationRecord>, String> {
    let limit = i64::try_from(limit).unwrap_or(20).clamp(1, 200);
    let mut statement = connection
        .prepare(
            "SELECT id, event_id, event_type, channel, status, title, body, reason, error_message, created_at, sent_at
             FROM notification_records
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .map_err(|error| format!("读取通知记录失败：{error}"))?;
    let rows = statement
        .query_map([limit], |row| {
            let event_type_text: String = row.get(2)?;
            let status_text: String = row.get(4)?;
            let created_at_text: String = row.get(9)?;
            let sent_at_text: Option<String> = row.get(10)?;
            let event_type = serde_json::from_str(&event_type_text)
                .map_err(|error| from_parse_error(2, error))?;
            let status =
                serde_json::from_str(&status_text).map_err(|error| from_parse_error(4, error))?;
            let channel = parse_channel_id(&row.get::<_, String>(3)?).map_err(|error| {
                from_parse_error(3, io::Error::new(io::ErrorKind::InvalidData, error))
            })?;
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
            Ok(NotificationRecord {
                id: row.get(0)?,
                event_id: row.get(1)?,
                event_type,
                channel,
                status,
                title: row.get(5)?,
                body: row.get(6)?,
                reason: row.get(7)?,
                error_message: row.get(8)?,
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

// SQLite 中使用稳定字符串作为唯一键，避免 enum 序列化格式变更影响去重。
pub fn channel_id(channel: &NotificationChannel) -> &'static str {
    match channel {
        NotificationChannel::Bark => "bark",
        NotificationChannel::Ntfy => "ntfy",
    }
}

pub fn channel_from_id(value: &str) -> Result<NotificationChannel, String> {
    parse_channel_id(value)
}

pub fn parse_notification_channel_configs(
    value: &Value,
) -> Result<Vec<NotificationChannelConfig>, String> {
    let list = value
        .get("channels")
        .and_then(|value| value.as_array())
        .ok_or_else(|| "channels 必须是数组".to_string())?;
    let mut channels = Vec::new();
    for item in list {
        let channel_text = item
            .get("channel")
            .and_then(|value| value.as_str())
            .ok_or_else(|| "channel 不能为空".to_string())?;
        let channel = match channel_text {
            "bark" => NotificationChannel::Bark,
            "ntfy" => NotificationChannel::Ntfy,
            _ => return Err("channel 仅支持 bark 或 ntfy".to_string()),
        };
        let enabled = match item.get("enabled") {
            Some(value) => value
                .as_bool()
                .ok_or_else(|| "enabled 必须是布尔值".to_string())?,
            None => false,
        };
        let payload = match item.get("payload") {
            Some(value) if value.is_object() => value.clone(),
            Some(_) => return Err("payload 必须是对象".to_string()),
            None => Value::Object(Default::default()),
        };
        channels.push(NotificationChannelConfig {
            channel,
            enabled,
            payload,
            // 配置由后端落库时统一刷新更新时间，调用方无需信任前端时间。
            updated_at: Utc::now(),
        });
    }
    Ok(channels)
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
