# NiuMa SQLite 通知历史持久化重设计

## 背景

当前项目已经将事件、会话、关注项、最新活动和去重缓存收敛为进程内运行态。SQLite 仍保留旧的状态库命名和多类数据表，导致 `state.sqlite` 的职责和当前设计不匹配。

本次设计选择方案 B：SQLite 只保存通知历史；配置改为 JSON 文件持久化；运行态继续保存在内存中。

## 目标

- 默认数据库文件从 `state.sqlite` 改为 `niuma.sqlite`。
- `niuma.sqlite` 只保存通知历史数据。
- 内置通知和插件通知合并到统一通知表。
- 配置数据不再放入 SQLite，改为 JSON 文件。
- 不迁移旧通知历史，不迁移旧配置。
- 不删除、不清空、不改写旧 `state.sqlite`。

## 非目标

- 不提供旧 `state.sqlite` 到 `niuma.sqlite` 的数据迁移。
- 不保留旧通知表和插件通知表的双表结构。
- 不把事件、会话、关注项、最新活动重新写回数据库。
- 不在本次设计中重做通知历史接口的前端交互。

## 持久化布局

应用数据目录保持使用现有平台目录，例如 macOS 下为：

```text
~/Library/Application Support/NiumaNotifier/
```

新布局：

```text
NiumaNotifier/
  niuma.sqlite
  config.json
  plugin-configs/
    <plugin_id>.json
```

`niuma.sqlite` 只负责通知历史。`config.json` 保存全局配置，`plugin-configs/*.json` 保存插件或通知器的独立配置。

## SQLite Schema

新库只创建一张通知表：

```sql
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
```

字段含义：

- `id`：通知记录主键。
- `notifier_id`：通知器标识，例如 `bark`、`ntfy` 或插件 ID。
- `notifier_type`：通知器类型，取值为 `builtin` 或 `plugin`。
- `event_id`：触发通知的事件 ID，用于同一通知器内去重。
- `event_type`：事件类型的稳定序列化值。
- `status`：通知状态，例如 `pending`、`sent`、`failed`、`skipped`。
- `title`、`body`、`reason`、`error_message`：通知展示和诊断字段。
- `created_at`、`sent_at`：创建时间和发送完成时间。

`UNIQUE(notifier_id, event_id)` 保证同一通知器对同一事件只保留一条记录。不同通知器可以对同一事件分别记录。

## 被移除的 SQLite 表

新 `niuma.sqlite` 不再创建以下表：

```text
sessions
attention_items
latest_activity
public_events
app_settings
plugin_configs
plugin_notification_results
```

其中事件、会话、关注项、最新活动和运行态状态继续使用内存结构；配置类数据由 JSON 文件负责。

## 配置文件

全局配置写入：

```text
config.json
```

建议结构：

```json
{
  "language_preference": "system",
  "listener_config": {
    "enabled": false
  },
  "plugin_enabled_map": {}
}
```

插件配置写入：

```text
plugin-configs/<plugin_id>.json
```

插件配置内容保持插件自己的 JSON payload，不额外引入数据库表。

运行态状态不写入配置文件。应用重启后运行态从空内存状态开始，由新的事件流重新建立。

## 命名调整

- 默认数据库路径从 `state.sqlite` 改为 `niuma.sqlite`。
- 环境变量从 `NIUMA_STATE_PATH` 改为 `NIUMA_DB_PATH`。
- 不兼容旧的 `NIUMA_STATE_PATH`。

代码命名可以分阶段收敛。第一阶段允许保留部分外层类型名以降低改动范围，但新代码和文档应避免继续使用“state path”表达 SQLite 数据库路径。

## 旧数据策略

本次设计不读取、不迁移、不删除旧 `state.sqlite`。

旧文件保留在磁盘上，作为用户历史数据的非破坏性保留物。新版本启动时直接使用 `niuma.sqlite`，并在需要时创建新的通知表和配置 JSON 文件。

## API 兼容

通知历史接口继续保留。底层从统一 `notification_records` 表读取，返回结构保持现有前端可用。

内置通知记录返回时可将 `notifier_id` 映射为现有的 `channel` 字段；插件通知记录返回时可同时提供 `channel = notifier_id` 和 `plugin_id = notifier_id`。这样前端不需要感知数据库表从双表合并为单表。

## 错误处理

- SQLite 初始化失败时返回现有错误链路，不静默降级。
- JSON 配置文件不存在时使用默认配置，并在保存时创建文件。
- JSON 配置解析失败时返回明确错误，不自动覆盖损坏文件。
- 创建 `plugin-configs/` 目录失败时返回明确错误。

## 测试范围

- 默认数据库路径应指向 `niuma.sqlite`。
- `NIUMA_DB_PATH` 可以覆盖默认数据库路径。
- `NIUMA_STATE_PATH` 不再影响数据库路径。
- 新 schema 只创建 `notification_records` 和相关索引。
- 内置通知和插件通知写入同一张表。
- 同一 `notifier_id + event_id` 去重，不同通知器可以记录同一事件。
- 通知历史接口可以按时间倒序返回内置通知和插件通知。
- 配置读写走 JSON 文件，不再依赖 `app_settings` 或 `plugin_configs`。
- 旧 `state.sqlite` 不被读取、不被删除、不被改写。

## 实施顺序

1. 调整数据库路径配置和环境变量命名。
2. 收缩 SQLite schema，只创建统一通知表。
3. 合并通知存储模型，移除插件通知结果独立表。
4. 引入 JSON 配置存储，替换 SQLite 配置读写。
5. 更新文档中 `NIUMA_STATE_PATH` 的描述。
6. 补充和调整测试，验证旧库不参与新流程。
