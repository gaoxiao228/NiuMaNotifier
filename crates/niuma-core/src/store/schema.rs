use rusqlite::Connection;

pub(super) fn init_schema(connection: &Connection) -> Result<(), String> {
    // 新库只持久化通知历史；事件、运行态条目、关注项和配置分别由内存/JSON 负责。
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS notification_records (
                id TEXT PRIMARY KEY,
                notifier_id TEXT NOT NULL,
                notifier_type TEXT NOT NULL,
                event_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                status TEXT NOT NULL,
                title TEXT,
                body TEXT,
                reason TEXT,
                error_message TEXT,
                created_at TEXT NOT NULL,
                sent_at TEXT,
                UNIQUE(notifier_id, event_id)
            );

            CREATE INDEX IF NOT EXISTS idx_notification_records_created_at
                ON notification_records(created_at);
            CREATE INDEX IF NOT EXISTS idx_notification_records_notifier_created_at
                ON notification_records(notifier_id, created_at);
            ",
        )
        .map_err(|error| format!("初始化 SQLite 通知库失败：{error}"))
}
