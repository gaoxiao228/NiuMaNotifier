use rusqlite::Connection;

pub(super) fn init_schema(connection: &Connection) -> Result<(), String> {
    // 首版 schema 直接创建最终结构：关键列用于排序/去重，payload 保留完整业务对象。
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                tool TEXT NOT NULL,
                project_path TEXT NOT NULL,
                project_name TEXT NOT NULL,
                status TEXT NOT NULL,
                last_event_id TEXT,
                last_activity_at TEXT NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS attention_items (
                event_id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                status TEXT NOT NULL,
                attention_resolve_key TEXT,
                created_at TEXT NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS latest_activity (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                event_id TEXT,
                session_id TEXT,
                status TEXT NOT NULL,
                updated_at TEXT,
                payload TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS public_events (
                id TEXT PRIMARY KEY,
                dedupe_key TEXT NOT NULL,
                source TEXT NOT NULL,
                tool TEXT NOT NULL,
                session_id TEXT NOT NULL,
                project_path TEXT NOT NULL,
                project_name TEXT NOT NULL,
                event_type TEXT NOT NULL,
                severity TEXT NOT NULL,
                created_at TEXT NOT NULL,
                payload TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS notification_records (
                id TEXT PRIMARY KEY,
                event_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                channel TEXT NOT NULL,
                status TEXT NOT NULL,
                title TEXT,
                body TEXT,
                reason TEXT,
                error_message TEXT,
                created_at TEXT NOT NULL,
                sent_at TEXT,
                UNIQUE(event_id, channel)
            );
            CREATE TABLE IF NOT EXISTS plugin_notification_results (
                id TEXT PRIMARY KEY,
                plugin_id TEXT NOT NULL,
                event_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                status TEXT NOT NULL,
                title TEXT,
                body TEXT,
                reason TEXT,
                error_message TEXT,
                created_at TEXT NOT NULL,
                sent_at TEXT,
                UNIQUE(plugin_id, event_id)
            );
            CREATE TABLE IF NOT EXISTS app_settings (
                key TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS plugin_configs (
                plugin_id TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_sessions_last_activity_at
                ON sessions(last_activity_at);
            CREATE INDEX IF NOT EXISTS idx_attention_items_created_at
                ON attention_items(created_at);
            CREATE INDEX IF NOT EXISTS idx_public_events_created_at
                ON public_events(created_at);
            CREATE INDEX IF NOT EXISTS idx_notification_records_created_at
                ON notification_records(created_at);
            CREATE INDEX IF NOT EXISTS idx_plugin_notification_results_created_at
                ON plugin_notification_results(created_at);
            ",
        )
        .map_err(|error| format!("初始化 SQLite 状态库失败：{error}"))
}
