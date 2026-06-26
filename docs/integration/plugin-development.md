# Plugin Development Guide

This document describes the NiumaNotifier plugin v1 integration model. A plugin is a local trusted executable described by `plugin.json`, started and stopped by NiumaNotifier, and connected to the main app through the Local API.

NiumaNotifier currently supports these plugin shapes:

| Type | `kind` | Main capabilities | Description |
| --- | --- | --- | --- |
| Tool watcher plugin | `tool` | `event_watcher` | Watches raw state from tools such as Codex, Claude Code, or Cursor, converts it to unified `NiumaEvent` objects, and reports them to the main app. |
| Tool session provider plugin | `tool` | `tool_session_list_provider`, `tool_session_detail_provider` | Parses raw tool session files and provides discovered session lists and normalized message details to the host. |
| Notification plugin | `notification` | `event_consumer`, `notification_test`, optional `approval_handler` | Consumes the main app event stream, decides whether to send external notifications such as Bark or ntfy, and can handle approval decisions only when it declares `approval_handler`. |
| Status indicator plugin | `status_indicator` | `state_consumer` | Consumes the main state stream for external lights, status panels, desktop pets, or similar displays. |

## Plugin Boundaries

- Plugins are local trusted executables installed by the user and managed by NiumaNotifier.
- NiumaNotifier v1 does not provide strong sandboxing, signature verification, or a plugin marketplace.
- Plugins must not write NiumaNotifier persistent files directly. All state events and notification results must be reported through the Local API.
- Status indicator plugins do not need to understand event reporting or notification result protocols. They only consume the `/api/v1/state/stream` SSE main state stream.
- The Local API is intended for local trusted callers by default and does not include built-in authentication. If it is explicitly bound to a non-loopback address, protect it with external network policy.

## Development Flow

Recommended development order for an external plugin:

1. Write `plugin.json`, and first confirm that `id`, `kind`, `tool_id`, `capabilities`, and the current platform match.
2. Write a local executable that can start independently, and read the Local API URL and plugin ID from environment variables.
3. For a tool watcher plugin, implement `/api/v1/plugin-events` reporting first, and keep `dedupe_key` stable.
4. For a notification plugin, subscribe to `/api/v1/events/stream`, then implement notification sending, local dedupe, and notification result writeback.
5. For a status indicator plugin, only subscribe to `/api/v1/state/stream`. Do not report events or write notification history.
6. Place the plugin directory in the user plugin directory, restart NiumaNotifier, enable the plugin from the plugin management list, and observe its runtime status.

A minimal usable plugin should first be able to start, exit, and handle Local API failures. Add more complex behavior incrementally after that foundation works.

## Plugin Package Layout

Recommended layout:

```text
niuma-plugin-example/
  plugin.json
  bin/
    niuma-plugin-example
  assets/
    icon.png
```

The repository provides one minimal tool plugin example and one status indicator plugin example:

```text
examples/plugins/niuma-plugin-demo/
examples/plugins/status-indicator-demo/
```

Install to the local plugin directory:

```bash
mkdir -p "$HOME/Library/Application Support/NiumaNotifier/plugins"
cp -R examples/plugins/niuma-plugin-demo "$HOME/Library/Application Support/NiumaNotifier/plugins/"
```

After restarting NiumaNotifier, the listener list should show `Demo Tool`. Once enabled, the plugin reports a stable set of test events through `/api/v1/plugin-events`.

The status indicator plugin example is installed the same way. Once enabled, it consumes the main state through `/api/v1/state/stream` and prints indicator output.

User plugin directory:

```text
~/Library/Application Support/NiumaNotifier/plugins/<plugin-id>/plugin.json
```

External plugins cannot override built-in plugin IDs such as `builtin-codex`, `builtin-bark`, or `builtin-ntfy`.

## Runtime Lifecycle

NiumaNotifier periodically discovers `plugin.json` files in the plugin directory and manages long-running plugin processes based on enabled state:

1. Discovery: the main app loads built-in plugins and external plugins from the user plugin directory.
2. Enablement: `tool` plugins are controlled by tool listener switches. Plugins without `tool_id` are controlled by the general plugin enabled state.
3. Startup: plugins declaring `event_watcher`, `event_consumer`, `state_consumer`, `tool_session_list_provider`, or `tool_session_detail_provider` are started as long-running child processes.
4. Runtime: the main app injects environment variables into the plugin and sets the working directory to the directory that contains `plugin.json`.
5. Stop: when a plugin is disabled, its manifest changes, or it is removed, the main app terminates the old process.
6. Restart: if a plugin exits unexpectedly, the main app records the `failed` state and retries after a short backoff.

Plugin process requirements:

- Handle `SIGTERM` or the platform equivalent and exit promptly.
- Do not assume the process starts only once. After restart, recover local dedupe state from `NIUMA_PLUGIN_DATA_DIR`.
- Do not write runtime state into NiumaNotifier internal persistent files.
- `stdout` and `stderr` are not currently shown as user-visible logs. During development, write debug logs to the plugin data directory if needed.
- Plugins declaring `tool_session_list_provider` or `tool_session_detail_provider` use `stdout` as the provider JSON Lines RPC channel. These plugins must not write normal logs to `stdout`; write normal logs to `stderr` or a log file under `NIUMA_PLUGIN_DATA_DIR`.

Listener switch behavior:

- A `tool` plugin with `event_watcher` is controlled by that tool's listener switch.
- If the same process also declares `tool_session_list_provider` and `tool_session_detail_provider`, disabling the tool listener disables the provider as well.
- When a tool listener is disabled, the host clears the cached session snapshot and event cursor state for that tool. Reader plugins should expect `/api/v1/session_list` and `/api/v1/session_project_groups` to return no sessions for that tool.
- If a reader plugin asks for a detail that is no longer available because the provider is disabled or stopped, the host returns a business failure envelope instead of stale conversation content.

## Manifest

Tool watcher plugin example:

```json
{
  "id": "niuma-plugin-codex",
  "kind": "tool",
  "tool_id": "codex",
  "display_name": "Codex",
  "version": "0.1.0",
  "command": "./bin/niuma-plugin-codex",
  "args": [],
  "env": {},
  "platforms": ["macos"],
  "capabilities": ["event_watcher"],
  "icon_url": "./assets/icon.png"
}
```

Tool plugin example that provides both event watching and session provider capabilities:

```json
{
  "id": "niuma-plugin-codex",
  "kind": "tool",
  "tool_id": "codex",
  "display_name": "Codex",
  "version": "0.1.0",
  "command": "./bin/niuma-plugin-codex",
  "args": [],
  "env": {},
  "platforms": ["macos"],
  "capabilities": [
    "event_watcher",
    "tool_session_list_provider",
    "tool_session_detail_provider"
  ],
  "icon_url": "./assets/icon.png"
}
```

This combined tool plugin shape is the recommended shape used by the built-in `builtin-codex` plugin. One process can watch tool events and also provide session lists and details through provider RPC. The implementation must reserve `stdout` for provider RPC only; event watcher logs, debug output, and normal runtime logs must not be written to `stdout`.

Notification plugin example:

```json
{
  "id": "niuma-plugin-webhook",
  "kind": "notification",
  "display_name": "Webhook",
  "version": "0.1.0",
  "command": "./bin/niuma-plugin-webhook",
  "args": [],
  "env": {},
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["event_consumer", "notification_test"],
  "config_schema": [
    {
      "key": "url",
      "type": "url",
      "label": "Webhook URL",
      "required": true
    },
    {
      "key": "token",
      "type": "secret",
      "label": "Token",
      "required": false
    }
  ]
}
```

Status indicator plugin example:

```json
{
  "id": "status-indicator-demo",
  "kind": "status_indicator",
  "display_name": "Status Indicator Demo",
  "version": "0.1.0",
  "command": "node",
  "args": ["./bin/status-indicator-demo.mjs"],
  "env": {},
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["state_consumer"],
  "config_schema": [
    {
      "key": "style",
      "type": "select",
      "label": "Display style",
      "required": false,
      "default": "indicator",
      "options": ["indicator", "pet", "panel"]
    }
  ]
}
```

Field reference:

| Field | Required | Description |
| --- | --- | --- |
| `id` | Yes | Unique plugin ID. External plugin IDs cannot duplicate built-in plugin IDs. |
| `kind` | No | Plugin type. Defaults to `tool`. Supported values are `tool`, `notification`, and `status_indicator`. |
| `tool_id` | Required for tool plugins | Tool ID, such as `codex`, `claude_code`, `cursor`, or `demo_tool`. |
| `display_name` | Yes | Display name in the UI. |
| `version` | Yes | Plugin version. |
| `command` | Required for external plugins | Startup command. A relative command containing a path separator is resolved relative to the `plugin.json` directory. A bare command is resolved through the system `PATH`. |
| `args` | No | Startup arguments. Relative path arguments are not rewritten automatically, but the plugin working directory is set to the `plugin.json` directory. |
| `env` | No | Extra environment variables injected into the plugin process. |
| `platforms` | No | Supported platforms. Current values are `macos`, `windows`, and `linux`. An empty array means all platforms. |
| `capabilities` | No | Supported values are `event_watcher`, `event_consumer`, `approval_handler`, `notification_test`, `state_consumer`, `tool_session_list_provider`, `tool_session_detail_provider`, `tool_session_list_reader`, and `tool_session_detail_reader`. |
| `icon_url` | No | Icon URL or relative asset path. |
| `config_schema` | No | Plugin configuration field definitions for the UI and configuration API. |

Constraints:

- `tool` plugins must provide `tool_id`.
- Non-`tool` plugins cannot declare `event_watcher`.
- `event_watcher` plugins are started and stopped by the tool listener switch.
- If the same `tool` plugin declares both `event_watcher` and session provider capabilities, disabling the tool listener stops the whole plugin process. The session snapshot and detail provider for that tool become unavailable as well.
- Plugins without `tool_id` are started and stopped by the general plugin enabled state.
- Plugins declaring `event_watcher`, `event_consumer`, `state_consumer`, `tool_session_list_provider`, or `tool_session_detail_provider` are managed by the runtime manager as long-running child processes.
- `approval_handler` is an extra capability for approval decisions. It must be used together with `event_consumer`; `approval_handler` alone is not a valid runtime mode.
- `event_watcher`, `tool_session_list_provider`, and `tool_session_detail_provider` are independent capabilities. Tool watcher capability does not imply tool session provider capability.
- For the same `tool_id`, each provider capability can be declared by only one plugin. For example, there can be only one `event_watcher`, one `tool_session_list_provider`, and one `tool_session_detail_provider`.
- Non-`tool` plugins cannot declare provider capabilities. `tool_session_detail_provider` must be declared together with `tool_session_list_provider` in the same plugin.
- `tool_session_detail_reader` means the plugin can read AI conversation content. The plugin management UI displays it as a sensitive capability. In v1, this declaration is a development contract and display marker, not a server-enforced authentication boundary.

Capability reference:

| Capability | Plugin kind | Description |
| --- | --- | --- |
| `event_watcher` | `tool` | Watches raw tool events and reports them through `/api/v1/plugin-events`. |
| `event_consumer` | `notification` | Consumes the `/api/v1/events/stream` event stream. |
| `approval_handler` | `notification`, with `event_consumer` | Can submit approval decisions. |
| `notification_test` | `notification` | Supports the test notification button in the main UI. |
| `state_consumer` | `status_indicator` | Consumes the `/api/v1/state/stream` main state stream. |
| `tool_session_list_provider` | `tool` | Provides the discovered session list for the tool to the host. |
| `tool_session_detail_provider` | `tool` | Provides normalized message details by `session_id` to the host. |
| `tool_session_list_reader` | Any business plugin | Reads the host `session_list` API. |
| `tool_session_detail_reader` | Any business plugin | Reads AI conversation content through the host `session_detail` API. Sensitive. |

## Configuration Schema

`config_schema` supports the following field types:

| Type | JSON value type | Description |
| --- | --- | --- |
| `string` | string | Plain text. |
| `secret` | string | Sensitive text such as tokens or device keys. |
| `url` | string | URL text. |
| `number` | number | Number. |
| `boolean` | boolean | Switch. |
| `select` | string | Enum value. `options` can restrict allowed values. |

Configuration field structure:

| Field | Required | Description |
| --- | --- | --- |
| `key` | Yes | Configuration key. It cannot be empty and cannot be duplicated within the same plugin. |
| `type` | Yes | Configuration type. |
| `label` | Yes | UI display label. It cannot be empty. |
| `required` | No | Whether the field is required. |
| `default` | No | Default value. |
| `options` | No | Allowed values for `select`. |

When configuration is saved, the main app performs basic type validation and required-field validation according to `config_schema`. Unknown configuration keys are not currently rejected, but plugins should not depend on undeclared fields.

## Startup Environment Variables

NiumaNotifier injects the following environment variables when starting a plugin:

| Variable | Description |
| --- | --- |
| `NIUMA_LOCAL_API_URL` | Local API URL, for example `http://127.0.0.1:27874`. |
| `NIUMA_PLUGIN_ID` | Current plugin ID. |
| `NIUMA_TOOL_ID` | Tool ID for the current plugin. Only present for `tool` plugins. |
| `NIUMA_PLUGIN_CONFIG_PATH` | Plugin configuration file path. The main app currently writes this file for built-in Bark and ntfy notification plugins. External plugins should prefer the configuration API. |
| `NIUMA_PLUGIN_DATA_DIR` | Plugin data directory. Plugins can use it for local dedupe or other runtime state. |
| `NIUMA_PARENT_PID` | Main app process PID. Plugins can periodically check whether the process still exists. If it no longer exists, the plugin should exit to avoid orphan processes after a main app crash. |
| `NIUMA_DB_PATH` | SQLite notification history database path for the current instance. It is for diagnostics only and must not be written directly. |

External plugins should use `NIUMA_PARENT_PID` as a self-cleanup signal. If the variable is missing or malformed, the plugin should remain compatible and continue running. It should exit only after confirming that the parent process no longer exists.

## Configuration And Local Data

Plugin development should distinguish these data categories:

| Data | Recommended location | Description |
| --- | --- | --- |
| Plugin configuration | `/api/v1/plugins/config` | Validated and persisted by the main app according to `config_schema`. External plugins should read it through the Local API at runtime. |
| Plugin local runtime data | `NIUMA_PLUGIN_DATA_DIR` | Maintained by the plugin, such as notification dedupe records, reconnect state, window position, or debug logs. |
| Notification history | Local API writeback | Real notification and test notification results are written back through Local API endpoints and saved by the main app. |

Constraints:

- External plugins should not directly read or write `config.json`, `plugin-configs`, `niuma.sqlite`, or other internal main app files.
- `NIUMA_DB_PATH` is only a diagnostic path, not a plugin extension point.
- Events, runtime state items, attention items, and latest activity are in-memory runtime state in the main app. Plugins cannot depend on database queries for historical events.
- Notification plugins should store "already notified" local dedupe records in `NIUMA_PLUGIN_DATA_DIR`, then write the send result back through the Local API.

## Local API Contract

Except for SSE streams, plugin-related JSON APIs use this unified response shape:

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

Rules:

- `code = 0` means success.
- `code != 0` means failure.
- Business validation failures usually return `HTTP 200 + non-zero code`.
- Protocol errors such as invalid JSON return `HTTP 400 + non-zero code`.
- System errors return `HTTP 500 + non-zero code`.
- SSE streams are protocol exceptions and do not use the JSON envelope.

Common error codes:

| `code` | Meaning |
| --- | --- |
| `0` | Success. |
| `100004` | Request body cannot be parsed, or parameter format is invalid. |
| `100101` | Business validation failed, such as unknown plugin, plugin type mismatch, or configuration validation failure. |
| `900001` | System error. |
| `900005` | Route not found. |

Call recommendations:

- Even when a business API returns `HTTP 200`, always check `code`. Do not rely on HTTP status alone.
- Put business parameters for `GET` requests in query parameters, for example `/api/v1/plugins/config?plugin_id=...`.
- Use a JSON body for `POST` requests. On failure, read the outer `message` field for diagnostics.
- If the plugin starts before the Local API is available, retry for a limited time instead of exiting permanently.
- During debugging, verify APIs with `curl` before wiring them into the plugin process.

Read plugin configuration:

```bash
curl "$NIUMA_LOCAL_API_URL/api/v1/plugins/config?plugin_id=$NIUMA_PLUGIN_ID"
```

Report tool events:

```bash
curl -X POST "$NIUMA_LOCAL_API_URL/api/v1/plugin-events" \
  -H "Content-Type: application/json" \
  -d '{"plugin_id":"niuma-plugin-demo","events":[]}'
```

## Tool Session Reading

Third-party reader plugins read tool sessions through the host Local API. They must not read Codex, Claude Code, or other tool directories directly, and they must not call provider plugins directly. The tool session view is separate from Niuma runtime state: `/api/v1/runtime_state_list` returns Niuma state-machine runtime items, while raw tool session lists, project groups, and normalized message details use the endpoints below.

```http
GET /api/v1/session_list?tool=codex&include_subagents=false&active_only=false&limit=100
GET /api/v1/session_project_groups?tool=codex&project_path=/repo&include_subagents=false&page=1&page_size=20
GET /api/v1/session_detail?tool=codex&session_id=session-1&limit=100&cursor=cursor-1
```

`session_list` reads the latest provider snapshot stored by the host. Reader plugins do not scan disk. Common query parameters:

| Parameter | Default | Description |
| --- | --- | --- |
| `tool` | `all` | `codex`, `claude_code`, a custom tool ID, or `all`. |
| `include_subagents` | `false` | Whether to include subagent sessions. |
| `active_only` | `false` | Whether to return only active sessions. |
| `limit` | `100` | Maximum number of returned sessions. |

Successful `session_list` response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "tool": "codex",
        "session_id": "session-1",
        "project_path": "/repo",
        "project_name": "repo",
        "is_subagent": false,
        "normalized_session_id": "session-1",
        "status": "active"
      }
    ]
  }
}
```

`session_project_groups` groups the provider snapshot by project path and returns project -> normalized session -> optional raw session details. Normalized sessions collect subagents under `normalized_session_id`; raw subagent details are not expanded by default, but parent session `updated_at` still includes subagent activity. Each normalized session includes `updated_at` as the latest raw session update time, and may include `first_user_message_preview` / `first_user_message_at` for the earliest user message preview known to the provider. Project group counters use explicit names: `normalized_session_count` is the number of normalized sessions, `raw_session_count` is the number of raw session files, and `subagent_count` is the number of raw session files produced by subagents. Common query parameters:

| Parameter | Default | Description |
| --- | --- | --- |
| `tool` | `all` | `codex`, `claude_code`, a custom tool ID, or `all`. |
| `project_path` | empty | Exact project path filter. |
| `include_subagents` | `false` | Whether to expand raw session details under each normalized session. |
| `page` | `1` | Project group page number. |
| `page_size` | `20` | Project group page size, capped at `100`. |

Successful `session_project_groups` response uses the standard pagination shape:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "tool": "codex",
        "project_path": "/repo",
        "project_name": "repo",
        "normalized_session_count": 1,
        "raw_session_count": 2,
        "subagent_count": 1,
        "sessions": [
          {
            "normalized_session_id": "session-1",
            "primary_session_id": "session-1",
            "title": "session-session",
            "status": "active",
            "updated_at": "2026-06-24T10:00:00Z",
            "first_user_message_preview": "Summarize this project",
            "first_user_message_at": "2026-06-24T09:30:00Z",
            "latest_event_summary": null,
            "subagent_count": 1
          }
        ]
      }
    ],
    "page": 1,
    "page_size": 20,
    "total": 1
  }
}
```

`session_project_groups/stream` provides an SSE snapshot stream with the same query parameters as `session_project_groups`:

```http
GET /api/v1/session_project_groups/stream?tool=codex&project_path=/repo&include_subagents=true&page=1&page_size=20
```

The stream sends `event: session_project_groups` frames. The `data` payload uses the same pagination shape as `session_project_groups`, and each normalized session adds runtime overlay fields from Niuma runtime state: `runtime_status`, `runtime_last_event_id`, and `runtime_last_activity_at`. Raw sessions include the same runtime fields when `include_subagents=true`. `status` keeps its provider meaning (`active`, `inactive`, or `unknown`); use `runtime_status` for Niuma states such as `running`, `waiting_approval`, `waiting_input`, `completed`, `error`, `idle`, or `stale`.

The stream sends a full snapshot immediately after the connection is opened. When Niuma runtime state changes, the server recomputes the same query and sends another full snapshot only if the serialized content changed. The stream is a display-state API, so consumers should treat it like `/api/v1/state/stream`: reconnect by opening the stream again and accepting the first snapshot, not by trying to resume from an SSE id.

Example frame:

```text
event: session_project_groups
id: 2
data: {"list":[{"tool":"codex","project_path":"/repo","project_name":"repo","normalized_session_count":1,"raw_session_count":1,"subagent_count":0,"sessions":[{"normalized_session_id":"session-1","primary_session_id":"session-1","title":"session-session","status":"active","runtime_status":"waiting_approval","runtime_last_event_id":"event-1","runtime_last_activity_at":"2026-06-25T02:15:35Z","updated_at":"2026-06-25T02:15:35Z","first_user_message_preview":"Summarize this project","first_user_message_at":"2026-06-25T02:10:00Z","latest_event_summary":null,"subagent_count":0}]}],"page":1,"page_size":20,"total":1}
```

Runtime overlay rules:

- `runtime_status = null` means Niuma currently has no runtime-state record for that session.
- `status` and `runtime_status` are intentionally separate. Do not use provider `status` to decide whether the session is waiting for approval or input.
- Normalized session `runtime_status` is aggregated from the raw sessions under the same `normalized_session_id`, using this priority: `waiting_approval` / `waiting_input`, then `error`, `running`, `completed`, `idle`, `stale`, then `null`.
- With `include_subagents=true`, `raw_sessions[]` includes each raw session's own runtime overlay fields. With `include_subagents=false`, only the normalized session summary is returned.

If query parameters have invalid types, the stream endpoint returns the standard error envelope before establishing SSE, for example `HTTP 400` with `code = 100003`. If page bounds fail business validation, it returns `HTTP 200` with a non-zero business code.

`session_detail` reads normalized message details by `tool + session_id`. `messages` are returned newest-first, so `messages[0]` is the newest message in the current page. Use `next_cursor` to continue reading older messages. The first version supports these roles: `user`, `assistant`, `system`, `tool_call`, `tool_result`, `event`, and `unknown`. The built-in Codex provider filters automatically injected context such as `AGENTS.md` instructions and environment context from `messages`; these entries also do not count as `first_user_message_preview`.

If a session was started through managed `niuma codex` and has been bound to the native Codex session, `session_detail` returns controllability in `data.control`. Mobile clients, trusted LAN helper panels, and external consoles should read this field to decide whether resume or interrupt is supported. Do not hard-code control support from the tool name alone.

Example `control` field:

```json
{
  "available": true,
  "provider": "niuma_codex",
  "wrapper_session_id": "niuma_codex_xxx",
  "capabilities": [
    "answer_input",
    "approve",
    "reject",
    "send_instruction",
    "interrupt"
  ],
  "actions": [
    {
      "type": "answer_input",
      "transport": "local_api",
      "endpoint": "/api/v1/session-control/answer-input",
      "debug_command": null
    },
    {
      "type": "send_instruction",
      "transport": "local_api",
      "endpoint": "/api/v1/session-control/send-instruction",
      "debug_command": "niuma codex-send niuma_codex_xxx \"Continue\""
    },
    {
      "type": "interrupt",
      "transport": "local_api",
      "endpoint": "/api/v1/session-control/interrupt",
      "debug_command": "niuma codex-interrupt niuma_codex_xxx"
    }
  ]
}
```

Field notes:

| Field | Description |
| --- | --- |
| `available` | Whether this session is currently controllable through a Niuma control channel. |
| `provider` | Control-channel provider. Managed Codex sessions use `niuma_codex`. |
| `wrapper_session_id` | The `niuma codex` wrapper session ID. Control calls must pass it together with `session_id`. |
| `capabilities` | Supported control capabilities. `answer_input`, `approve`, and `reject` mean Niuma can handle waiting input and approval for this session; `send_instruction` and `interrupt` mean the session supports active resume and interrupt. |
| `actions` | Recommended invocation methods. Mobile clients and external panels should use the `endpoint` where `transport = local_api`; `debug_command` is for local terminal troubleshooting only. |

Send a new instruction:

```http
POST /api/v1/session-control/send-instruction
Content-Type: application/json
```

Request body:

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "wrapper_session_id": "niuma_codex_xxx",
  "content": "Continue"
}
```

Successful response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "sent": true,
    "wrapper_session_id": "niuma_codex_xxx",
    "result": {}
  }
}
```

Answer a waiting input request:

```http
POST /api/v1/session-control/answer-input
Content-Type: application/json
```

Request body:

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "wrapper_session_id": "niuma_codex_xxx",
  "request_id": "codex-input:niuma_codex_xxx:9",
  "answers": {
    "app_form": ["Tray resident (Recommended)"]
  }
}
```

Successful response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "answered": true,
    "wrapper_session_id": "niuma_codex_xxx",
    "request_id": "codex-input:niuma_codex_xxx:9",
    "state_cleared": true,
    "result": {}
  }
}
```

`answers` uses `Record<string, string[]>`. Each key must come from `interaction.schema.questions[].id` on the waiting input event. For option questions, submit the selected option `label`; for free-text questions, submit the entered text. Empty objects or empty arrays return a business failure.

Interrupt the current turn:

```http
POST /api/v1/session-control/interrupt
Content-Type: application/json
```

Request body:

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "wrapper_session_id": "niuma_codex_xxx"
}
```

Successful response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "interrupted": true,
    "wrapper_session_id": "niuma_codex_xxx",
    "result": {}
  }
}
```

Control endpoint rules:

- The current implementation supports only `tool = "codex"` sessions controlled by `provider = "niuma_codex"`.
- `session_id` and `wrapper_session_id` must point to the same bound managed session in the registry. Mismatches return a business failure.
- The managed session must be bound and its process must still be alive. Expired sessions, exited sessions, or unavailable control sockets return a business failure.
- Business failures use the standard envelope, usually `HTTP 200` with `code = 100101`; callers must inspect `code` and `message`.
- Waiting input events can submit structured answers through `interaction.endpoint = "/api/v1/session-control/answer-input"`. Approval still uses the existing `/api/v1/approval-decisions` flow. `send_instruction` and `interrupt` are only for active resume and interruption.

`session_detail/stream` provides an SSE snapshot stream for one exact session:

```http
GET /api/v1/session_detail/stream?tool=codex&session_id=session-1&limit=100
```

This stream must always specify both `tool` and `session_id`. It does not support global subscription. Use `/api/v1/events/stream` for global or filtered event subscriptions, and use `session_detail/stream` only when a UI is displaying one session detail panel.

The stream sends `event: session_detail` frames. The `data` payload uses the same shape as the `data` object returned by `/api/v1/session_detail`. The first frame is sent immediately after the connection is opened. Later frames are sent when matching runtime events indicate that the requested raw session or normalized session may have changed, and only when the serialized detail snapshot changed.

`cursor` is intentionally not supported on the stream endpoint. The stream watches the latest page only; use `/api/v1/session_detail?cursor=...` for historical paging.

If `tool` or `session_id` is missing or empty, the stream endpoint returns the standard business failure envelope before establishing SSE. Invalid `limit` types return the standard `HTTP 400` parameter-type envelope before SSE is established.

Successful responses still use the standard envelope:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "tool": "codex",
    "session_id": "session-1",
    "messages": []
  }
}
```

When the corresponding tool listener is disabled:

- `session_list` and `session_project_groups` return successful empty lists for that tool because the host clears the provider snapshot.
- `session_detail` returns a business failure envelope if the requested session is unavailable. Current implementations may report `session_id 不存在` after the snapshot has been cleared, or a provider-specific message such as `session_provider_disabled` while the provider process is still alive.
- Reader plugins should treat both cases as "session content is currently unavailable" and should not fall back to reading raw tool session files.

NiumaNotifier v1 does not use plugin tokens for API authentication. `tool_session_list_reader` and `tool_session_detail_reader` are development-contract capabilities, UI display markers, and future authentication hooks. `tool_session_detail_reader` covers AI conversation content and should be displayed as sensitive in plugin management. Plugins should still connect only to the trusted local Local API.

## Tool Session Provider RPC

Tool plugins that declare `tool_session_list_provider` and `tool_session_detail_provider` are managed by the host as long-running processes. The host communicates with the provider through the plugin process `stdin/stdout` using JSON Lines:

- The host writes one JSON request line to plugin `stdin`.
- The plugin writes one JSON response line to `stdout`.
- The plugin may also write notifications without `id` to `stdout` to tell the host that the snapshot changed.
- `stdout` must contain provider JSON Lines only. Normal logs must go to `stderr` or a log file under `NIUMA_PLUGIN_DATA_DIR`.
- Each line must be complete JSON. Do not use pretty-printed JSON, multi-line JSON, or prefixed log text.

Request shape:

```json
{
  "id": "req-1",
  "method": "session_snapshot",
  "params": {}
}
```

Response shape:

```json
{
  "id": "req-1",
  "result": {}
}
```

Failure response:

```json
{
  "id": "req-1",
  "error": {
    "code": "session_not_found",
    "message": "session_id not found: session-1"
  }
}
```

Provider RPC failures are provider-level failures, not Local API envelopes. The host converts provider errors into the standard Local API response shape when a reader calls `/api/v1/session_detail`. Common provider error codes:

| Error code | Meaning |
| --- | --- |
| `method_not_found` | Unknown provider method. |
| `invalid_params` | Request parameters cannot be parsed or do not match the expected tool. |
| `session_not_found` | The requested raw `session_id` does not exist in the provider snapshot or file index. |
| `stale_session_file` | The raw file changed, was truncated, or no longer matches the indexed session. |
| `session_provider_disabled` | The corresponding tool listener is disabled, so session list/detail is intentionally unavailable. |
| `provider_internal_error` | Unexpected provider failure. |

Current provider methods:

| Method | Params | Result | Description |
| --- | --- | --- | --- |
| `session_snapshot` | `{ "tool": "codex" }` | `{ "tool": "codex", "sessions": [...] }` | Returns the lightweight session list currently discovered by the provider. |
| `session_detail` | `{ "tool": "codex", "session_id": "session-1", "limit": 100, "cursor": null }` | `{ "detail": {...} }` | Returns normalized message details for the given raw `session_id`. |

Current provider notification:

```json
{
  "method": "session_snapshot_updated",
  "params": {
    "tool": "codex",
    "sessions": []
  }
}
```

After receiving `session_snapshot_updated`, the host updates its in-memory session registry. `/api/v1/session_list` and `/api/v1/session_project_groups` both read this latest snapshot. Providers should notify only when the snapshot meaningfully changes to avoid high-frequency redundant refreshes.

`ToolSessionListItem` field semantics:

| Field | Description |
| --- | --- |
| `id` | Provider-side list item ID. Recommended value is `<tool>:<session_id>`. |
| `tool` | Tool ID. It must match the plugin manifest `tool_id`. |
| `session_id` | Raw tool session ID. `session_detail` uses this ID to locate details. |
| `project_path` / `project_name` | Project path and display name. Use an empty string or tool name when unknown. |
| `file_path` | Raw session file path. If the tool has no file, use a diagnostic source identifier. |
| `modified_at` | Raw session last modified time, or equivalent update time. |
| `discovered_at` / `last_seen_at` | When the provider discovered and last saw the session. |
| `is_active` / `status` | Provider active-state judgment. If unknown, `status` can be `unknown`. |
| `is_subagent` | Whether this is a subagent session. |
| `parent_session_id` | Raw tool parent session ID, if known. |
| `normalized_session_id` | Niuma-computed business session ID. Subagents usually normalize to a parent or root session. |
| `session_scope` | `main` or `subagent`. |
| `agent_nickname` / `agent_role` | Tool-provided subagent display fields, if any. |
| `normalization_status` | `resolved`, `parent_missing`, or `parent_unresolved`. Diagnostic only. |
| `first_user_message_preview` | Optional preview of the earliest user message. Providers should keep it short; the built-in Codex provider caps it at 200 characters. |
| `first_user_message_at` | Timestamp of `first_user_message_preview`, if known. |

`ToolSessionDetail` reuses the same identity fields and adds:

| Field | Description |
| --- | --- |
| `messages` | Current page of messages, newest-first. |
| `next_cursor` | Cursor for older messages. Empty means there is no next page. |

Provider implementation guidance:

- Keep snapshot indexes lightweight. Do not keep full conversation bodies in memory long-term.
- `session_detail` should read by `limit` pages. The host normalizes and caps `limit` before calling the provider.
- Cursors should point to stable line numbers, message sequence numbers, or raw tool offsets, so appended messages do not cause duplicate or missing pages.
- If a raw file is truncated, replaced, or changed, rebuild the index. Do not use an old cursor to read the wrong session.
- Keep `session_id` as the raw session ID for subagents. Do not replace it with the parent ID. Use `normalized_session_id` for aggregation.
- When the corresponding tool listener is disabled, the host stops the provider process and clears the tool snapshot. Reader plugins must not assume old session lists remain readable after the listener is disabled.
- If the provider process stays alive in a combined watcher/provider runtime while the listener is disabled, `session_snapshot` should return an empty session list and `session_detail` should fail with `session_provider_disabled`.
- Combined watcher/provider plugins should share one file repository or equivalent cache when possible, but must keep event projection separate from session reading. Provider code should not emit `NiumaEvent` objects directly; watcher code should continue to report events through `/api/v1/plugin-events`.

## Tool Event Reporting

`event_watcher` tool plugins report events through the Local API:

```http
POST /api/v1/plugin-events
Content-Type: application/json
```

Request body:

```json
{
  "plugin_id": "niuma-plugin-codex",
  "events": [
    {
      "id": "event-1",
      "dedupe_key": "codex:session-1:approval-1",
      "source": "plugin:niuma-plugin-codex",
      "tool": "codex",
      "session_id": "session-1",
      "parent_session_id": null,
      "normalized_session_id": "session-1",
      "session_scope": "main",
      "agent_nickname": null,
      "agent_role": null,
      "project_path": "/path/to/project",
      "project_name": "project",
      "event_type": "approval_requested",
      "severity": "urgent",
      "summary": "Bash: cargo test",
      "content": "Bash: cargo test",
      "error_message": null,
      "completion_reason": null,
      "failure_reason": null,
      "created_at": "2026-06-18T12:00:00Z"
    }
  ]
}
```

Success response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "plugin_id": "niuma-plugin-codex",
    "event_count": 1,
    "applied_count": 1,
    "session_count": 1
  }
}
```

Constraints:

- `plugin_id` must match a discovered plugin.
- The plugin must have an associated `tool_id`; otherwise it cannot report tool events through this endpoint.
- `event.tool` must equal the `tool_id` in the plugin manifest.
- `dedupe_key` must be stable. Repeated scans of the same raw event should keep the same value.
- After a successful report, NiumaNotifier writes the state through `StateMutationService` and triggers SSE updates.
- `applied_count = 0` usually means the event was deduplicated. The plugin does not need to retry the same event.

## NiumaEvent Fields

| Field | Required | Description |
| --- | --- | --- |
| `id` | Yes | Unique event ID. It should include the tool, session, and raw event identifier. |
| `dedupe_key` | Yes | Dedupe key. It must remain stable when scanning the same raw event repeatedly. |
| `source` | Yes | Source. Recommended value is `plugin:<plugin-id>`. |
| `tool` | Yes | Tool ID. It must match the plugin `tool_id`. |
| `session_id` | Yes | Tool-side session ID. |
| `parent_session_id` | No | Raw tool parent session ID. Used to describe parent relationship in subagent scenarios. |
| `normalized_session_id` | No | Niuma-computed business session ID. For main sessions this usually equals `session_id`; for subagents it usually points to a parent or root session. |
| `session_scope` | No | Session scope. Current recommended values are `main` and `subagent`. |
| `agent_nickname` | No | Tool-provided subagent display nickname. |
| `agent_role` | No | Tool-provided subagent role. |
| `project_path` | Yes | Project path. Use an empty string if unknown. |
| `project_name` | Yes | Project name. Use a readable tool-side name if unknown. |
| `event_type` | Yes | Event type. See the table below. |
| `severity` | Yes | Display severity. Common values are `info`, `urgent`, and `error`. |
| `summary` | Yes | Short summary. |
| `content` | No | Displayable body text or command content. |
| `error_message` | No | Error detail, preferred for error states. |
| `completion_reason` | No | Completion reason. |
| `failure_reason` | No | Failure reason. |
| `interaction` | No | Interaction details for waiting approval/input events. Consumers must use this field to decide whether and how the event can be handled. |
| `created_at` | Yes | Event occurrence time in RFC 3339 / ISO 8601 format. |

Supported `event_type` values:

| Value | State semantics |
| --- | --- |
| `session_started` | Session started. State becomes `running`. |
| `session_idled` | Session idle. State becomes `idle`. |
| `approval_requested` | Waiting for approval. State becomes `waiting_approval`. |
| `approval_resolved` | Approval was allowed or denied by a consumer. State returns to `running`. |
| `approval_returned_to_codex` | Niuma's proxy window ended. State remains `waiting_approval`, and the user must handle it in Codex. |
| `input_requested` | Waiting for input. State becomes `waiting_input`. |
| `task_failed` | Task failed. State becomes `error`. |
| `assistant_message_completed` | Assistant message completed. State becomes `completed`. |
| `manual_dismissed` | Manually dismissed. State becomes `completed` and attention items are cleared. |
| `session_staled` | Session became stale. Internal cleanup state. |
| `session_activity` | Normal activity. State becomes `running`. |

`interaction` describes whether a waiting event can be handled by a consumer:

| Field | Description |
| --- | --- |
| `kind` | Interaction kind: `approval` or `input`. |
| `handling` | Handling owner: `niuma` means it can be handled through the Niuma API, `tool` means the user must return to the original tool, and `none` means display-only. |
| `actionable` | Whether the consumer may show action controls. |
| `request_id` | Request ID when `actionable=true`. |
| `actions` | Available actions, for example `allow` and `deny` for approvals. |
| `endpoint` | Local API endpoint for submitting the action. |
| `schema` | Optional structured input schema. For Niuma-handled `kind = "input"` events, it contains `questions`. |
| `message` | User-facing instruction when the event is not actionable. |

Consumer rules:

- When `interaction.actionable = true`, render controls from `interaction.actions` and submit them to `interaction.endpoint`.
- When `interaction.kind = "input"`, `interaction.handling = "niuma"`, and `interaction.actionable = true`, consumers may render `interaction.schema.questions` and submit `answers: Record<string, string[]>` to `interaction.endpoint`.
- When `interaction.actionable = false`, show `interaction.message` only; do not show allow/deny or submit controls.
- `payload_ref` and `attention_resolve_key` are host-internal correlation/cleanup fields and are not part of the approval handling contract.

Example `interaction.schema.questions`:

```json
{
  "questions": [
    {
      "id": "app_form",
      "question": "How should this app primarily run?",
      "options": [
        {
          "label": "Tray resident (Recommended)",
          "description": "Runs in the background and is suitable for long-running monitoring."
        }
      ]
    }
  ]
}
```

Supported `completion_reason` values:

```text
normal
interrupted
rolled_back
aborted_unknown
```

Supported `failure_reason` values:

```text
timeout
context_window_exceeded
usage_limit_reached
server_overloaded
policy_blocked
response_stream_failed
connection_failed
quota_exceeded
internal_server_error
retry_limit
sandbox_error
fatal
unknown
```

## SSE Client Requirements

Notification plugins and status indicator plugins both consume real-time data through SSE. A shared SSE client implementation is recommended:

- Send `Accept: text/event-stream`.
- Ignore keep-alive comment lines that start with `:`.
- Split SSE frames by blank lines, and support multiple `data:` lines in the same frame.
- Dispatch by `event:`. Ignore unknown event types.
- Do not treat `curl -N` visibility as complete integration verification. The plugin's own SSE client must parse a complete `data: JSON` payload and actually dispatch the event to its handler.
- In the current v1 event stream, one event is usually delivered as one complete JSON object on a single `data:` line. Clients may try to parse as soon as a `data:` line is received; if parsing fails, keep accumulating following `data:` lines until the blank-line frame boundary and parse again.
- Reconnect automatically after disconnection. A fixed 2 to 5 second interval or exponential backoff is recommended.
- Do not assume SSE will replay history. Events missed during disconnection should not be recovered through database queries.

SSE currently has no authentication. Some streams expose query filters for narrowing delivered frames. External plugins should connect only to the trusted local Local API and should not expose it to the public internet.

## Notification Plugin Event Consumption

`event_consumer` notification plugins should subscribe to the real-time event stream:

```http
GET /api/v1/events/stream
Accept: text/event-stream
```

The stream accepts optional query filters for regular `event` frames:

```http
GET /api/v1/events/stream?tool=codex&session_id=session-1&event_type=approval_requested
GET /api/v1/events/stream?normalized_session_id=main-session&project_path=/repo
```

Supported filters are `tool`, `session_id`, `normalized_session_id`, `project_path`, `event_type`, and `severity`. Multiple filters are combined with AND semantics. These filters apply only to regular `event` frames; `notification_test` remains a control event for plugin testing and is not tied to a session.

Filter parameters:

| Parameter | Match target | Notes |
| --- | --- | --- |
| `tool` | `event.tool` | Tool id such as `codex`, `claude_code`, or a custom tool id. |
| `session_id` | `event.session_id` | Matches the raw session that produced the event. Use this when a plugin is attached to one exact session. |
| `normalized_session_id` | `event.normalized_session_id` | Matches the grouped/main session id. Prefer this for project group UIs that need main-session plus subagent events. Events without `normalized_session_id` do not match this filter. |
| `project_path` | `event.project_path` | Exact path match. URL-encode spaces and non-ASCII path characters. |
| `event_type` | `event.event_type` | Snake-case event type, for example `approval_requested`, `input_requested`, or `assistant_message_completed`. Invalid enum values return the standard `HTTP 400` parameter-type error before SSE is established. |
| `severity` | `event.severity` | Exact string match. Current built-in sources commonly use values such as `urgent`, but plugins should not assume a closed enum. |

Use filtered streams to reduce plugin-side work when the consumer has a narrow responsibility. For example, an approval-only consumer can subscribe to:

```http
GET /api/v1/events/stream?event_type=approval_requested
```

A session detail panel that only cares about one grouped session can subscribe to:

```http
GET /api/v1/events/stream?normalized_session_id=main-session
```

Filtering is not a permission boundary. It only changes what this SSE connection receives; it does not grant or restrict access to other Local API endpoints.

Normal event format:

```text
event: event
id: event-1
data: {"id":"event-1","tool":"codex","session_id":"session-1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","created_at":"2026-06-18T12:00:00Z"}
```

Codex subagent events may additionally include `parent_session_id`, `normalized_session_id`, `session_scope`, `agent_nickname`, and `agent_role`. `session_id` remains the actual source session. Business aggregation, approval arbitration, and default notification policy should prefer `normalized_session_id` and `session_scope`.

Test notification event format:

```text
event: notification_test
id: manual-test:builtin-ntfy:1
data: {"test_id":"manual-test:builtin-ntfy:1","plugin_id":"builtin-ntfy","title":"Test notification","body":"This is a test notification","created_at":"2026-06-18T12:00:00Z"}
```

Consumption constraints:

- `/api/v1/events/stream` only broadcasts newly applied events. It does not replay historical events.
- Duplicate reports that are deduplicated do not enter this stream.
- If filters are present, non-matching events are skipped and are not buffered for later delivery.
- Plugins should decide which events require notification.
- Default notification behavior should skip `assistant_message_completed` when `session_scope = "subagent"`, so a subagent finish is not reported as the main task finish. `approval_requested` should still notify because subagent approvals also need user action.
- Plugins should store local dedupe state in `NIUMA_PLUGIN_DATA_DIR` to avoid duplicate sends after reconnecting.
- SSE keep-alive comment lines should be ignored.

Recommended notification flow:

1. Receive an `event` from SSE.
2. Decide whether to notify based on `event_type`, `severity`, project name, or plugin configuration.
3. Check local dedupe state using `plugin_id + event.id` or a more specific business key.
4. Call the external notification service.
5. Whether sending succeeds or fails, call `/api/v1/plugins/notification-results` to write the result back.
6. After a successful send, update the local dedupe record. Failed sends can be retried according to the plugin policy, but avoid endless retry spam.

Notification plugins do not need to query the recent events list or write the notification history database directly.

## Approval Consumers

Consumers that can handle approvals must declare both `event_consumer` and `approval_handler`. Real-time approval UI must be triggered only by the `/api/v1/events/stream` stream when it sends `event: event` with `event_type = approval_requested`. Event consumers without `approval_handler` may notify that an approval is pending, but should not show approval actions or submit decisions.

`/api/v1/state/stream` and `/api/v1/main-state` are display-state APIs only. They may show that the app is in `waiting_approval`, but they must not be used to trigger Allow/Deny UI. `GET /api/v1/approval-requests?status=pending` is only for optional plugin startup recovery; each plugin decides whether it needs that recovery path.

Approval handling plugins can continue to use `kind = notification`. `notification_test` is not required; declare it only when the plugin needs to support the app's test-notification button.

Approval handling plugin manifest example:

```json
{
  "id": "niuma-plugin-approval-demo",
  "kind": "notification",
  "display_name": "Approval Demo",
  "version": "0.1.0",
  "command": "node",
  "args": ["./bin/approval-demo.mjs"],
  "env": {},
  "platforms": ["macos", "windows", "linux"],
  "capabilities": ["event_consumer", "approval_handler"]
}
```

`approval_requested` event example:

```text
event: event
id: event-approval-1
data: {"id":"event-approval-1","dedupe_key":"approval_requested:codex:s1:t1:Bash:abc123","source":"approval-api","tool":"codex","session_id":"session-1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","content":"Bash: cargo test","error_message":null,"interaction":{"kind":"approval","handling":"niuma","actionable":true,"request_id":"codex:s1:t1:Bash:abc123","actions":["allow","deny"],"endpoint":"/api/v1/approval-decisions"},"created_at":"2026-06-18T12:00:00Z"}
```

Consumers must use `interaction` to decide whether the event can be handled. Show approval actions only when `interaction.handling = "niuma"` and `interaction.actionable = true`, then submit decisions with `interaction.request_id`.

Parsing example:

```js
function approvalInteraction(event) {
  const interaction = event.interaction
  if (event.event_type !== 'approval_requested') return null
  if (interaction?.kind !== 'approval') return null
  return interaction
}
```

Submit a decision:

```http
POST /api/v1/approval-decisions
Content-Type: application/json
```

```json
{
  "request_id": "codex:s1:t1:Bash:abc123",
  "decision": "allow",
  "decided_by": "niuma-plugin-approval-demo",
  "decided_source": "plugin",
  "reason": "User allowed from notification"
}
```

Field rules:

| Field | Required | Description |
| --- | --- | --- |
| `request_id` | Yes | Use `interaction.request_id` from the approval event. |
| `decision` | Yes | `allow` or `deny`. |
| `decided_by` | Yes | Recommended value is the `NIUMA_PLUGIN_ID` environment variable. |
| `decided_source` | Yes | Recommended stable source labels include `plugin`, `notification`, `menu_bar`, `webhook`, or `mobile`. |
| `reason` | No | Human-readable reason that other consumers can display. |

Successful response when this consumer wins the decision:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "request_id": "codex:s1:t1:Bash:abc123",
    "accepted": true,
    "status": "allowed",
    "decision": "allow",
    "decided_by": "niuma-plugin-approval-demo",
    "decided_source": "plugin",
    "reason": "User allowed from notification",
    "proxy_status": "active"
  }
}
```

Successful response when another consumer handled it first:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "request_id": "codex:s1:t1:Bash:abc123",
    "accepted": false,
    "status": "denied",
    "decision": "deny",
    "decided_by": "dashboard",
    "decided_source": "ui",
    "reason": "User denied from the desktop UI",
    "proxy_status": "active"
  }
}
```

Business failure example:

```json
{
  "code": 200001,
  "message": "request_id cannot be empty",
  "data": null
}
```

After a request reaches business logic, the API usually returns HTTP 200. Plugins must check the top-level `code`. `code = 0` means the business operation succeeded. `accepted=true` means this consumer won the decision. `accepted=false` means another consumer or the desktop UI already handled it first. In that case, mark the local action as handled and do not retry to overwrite it.

Consumers may optionally recover pending approvals once on startup and may poll decision state:

```http
GET /api/v1/approval-requests?status=pending
GET /api/v1/approval-decisions?request_id=codex:s1:t1:Bash:abc123
```

Pending list response example:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "id": "codex:s1:t1:Bash:abc123",
        "tool": "codex",
        "session_id": "session-1",
        "turn_id": "turn-1",
        "tool_name": "Bash",
        "command": "cargo test",
        "description": "Allow running cargo test?",
        "project_path": "/repo",
        "project_name": "repo",
        "status": "pending",
        "decided_by": null,
        "decided_source": null,
        "reason": null,
        "proxy_status": "active"
      }
    ]
  }
}
```

Single decision query response example:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "request_id": "codex:s1:t1:Bash:abc123",
    "status": "allowed",
    "decision": "allow",
    "decided_by": "niuma-plugin-approval-demo",
    "decided_source": "plugin",
    "reason": "User allowed from notification",
    "proxy_status": "active"
  }
}
```

`GET /api/v1/approval-decisions` is for state lookup only and does not include `accepted`. Only the submit result from `POST /api/v1/approval-decisions` includes `accepted`.

Recommended recovery flow:

1. Read `NIUMA_LOCAL_API_URL` and `NIUMA_PLUGIN_ID` on startup.
2. Connect to `/api/v1/events/stream`.
3. If the plugin wants startup recovery, optionally call `GET /api/v1/approval-requests?status=pending` once as startup compensation.
4. Locally dedupe `request_id` values from SSE events and the pending list.
5. Present pending approval actions from `approval_requested` events and, if enabled, the startup pending list.
6. On `approval_resolved`, remove or disable the local action.
7. On `approval_returned_to_codex`, disable the action and tell the user to handle it in Codex.
8. After submitting a decision, use `accepted` to decide whether this consumer won the decision.

Event handling rules:

- On `approval_resolved`: disable local Allow/Deny actions and show the `decided_by` / `decided_source` handler.
- On `approval_returned_to_codex`: disable local Allow/Deny actions and tell the user to handle the request in Codex.
- Only `pending` approvals can be decided. Treat `allowed`, `denied`, and `returned_to_codex` as already handled.

Minimal Node.js consumer example:

```js
const apiUrl = process.env.NIUMA_LOCAL_API_URL
const pluginId = process.env.NIUMA_PLUGIN_ID

if (!apiUrl || !pluginId) {
  throw new Error('NIUMA_LOCAL_API_URL and NIUMA_PLUGIN_ID are required')
}

function approvalInteraction(event) {
  const interaction = event.interaction
  if (event.event_type !== 'approval_requested') return null
  if (interaction?.kind !== 'approval') return null
  if (interaction.handling !== 'niuma' || !interaction.actionable) return null
  return interaction
}

async function decide(requestId, decision) {
  // In v1, decisions are submitted through the local Local API; always check code.
  const response = await fetch(`${apiUrl}/api/v1/approval-decisions`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      request_id: requestId,
      decision,
      decided_by: pluginId,
      decided_source: 'plugin',
      reason: `User selected ${decision} in ${pluginId}`
    })
  })
  const body = await response.json()
  if (body.code !== 0) {
    throw new Error(body.message)
  }
  return body.data
}

async function connect() {
  // Event consumers receive approval requested/resolved/returned events over SSE.
  const response = await fetch(`${apiUrl}/api/v1/events/stream`, {
    headers: { Accept: 'text/event-stream' }
  })
  const reader = response.body.getReader()
  const decoder = new TextDecoder()
  let buffer = ''
  let currentEventName = 'message'
  let currentDataLines = []

  function dispatchCurrentData() {
    if (currentEventName !== 'event' || currentDataLines.length === 0) return
    const dataText = currentDataLines.join('\n')
    const event = JSON.parse(dataText)
    const interaction = approvalInteraction(event)
    if (!interaction?.request_id) return
    console.log(`[approval] ${event.project_name}: ${event.summary}`)
    console.log(`Call decide("${interaction.request_id}", "allow") or decide("${interaction.request_id}", "deny")`)
  }

  function resetCurrentFrame() {
    currentEventName = 'message'
    currentDataLines = []
  }

  function tryDispatchCurrentData() {
    try {
      dispatchCurrentData()
      resetCurrentFrame()
    } catch (error) {
      if (!(error instanceof SyntaxError)) throw error
    }
  }

  while (true) {
    const { value, done } = await reader.read()
    if (done) break
    buffer += decoder.decode(value, { stream: true })
    const lines = buffer.split(/\r?\n/)
    buffer = lines.pop() || ''
    for (const line of lines) {
      if (line.startsWith(':')) continue
      if (line === '') {
        // Blank lines end an SSE frame. Retry dispatch in case data spanned lines.
        tryDispatchCurrentData()
        resetCurrentFrame()
        continue
      }
      if (line.startsWith('event:')) {
        currentEventName = line.slice(6).trim()
        continue
      }
      if (line.startsWith('data:')) {
        currentDataLines.push(line.slice(5).trimStart())
        // v1 usually sends a complete JSON object in one data line.
        tryDispatchCurrentData()
      }
    }
  }
}

connect().catch((error) => {
  console.error(error)
  process.exit(1)
})
```

NiumaNotifier v1 does not use plugin tokens for API authentication, and the app does not treat `decided_by` as a verified security identity. `approval_handler` is a plugin development contract and capability display marker, not a server-enforced security boundary. Plugins should still connect only to the trusted local Local API.

## Status Indicator Main State Consumption

`state_consumer` status indicator plugins should subscribe to the main state stream:

```http
GET /api/v1/state/stream
Accept: text/event-stream
```

Main state event format:

```text
event: state
id: 12
data: {"version":12,"status":"waiting_approval","updated_at":"2026-06-18T12:00:00Z","session":{"id":"session-1","tool":"codex","project_name":"repo","project_path":"/repo"},"detail":{"event_id":"event-1","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","content":"Bash: cargo test","error_message":null,"interaction":{"kind":"approval","handling":"niuma","actionable":true,"request_id":"codex:s1:t1:Bash:abc123","actions":["allow","deny"],"endpoint":"/api/v1/approval-decisions"},"completion_reason":null,"failure_reason":null}}
```

Supported `status` values:

```text
idle
running
waiting_approval
waiting_input
completed
error
```

Consumption constraints:

- `/api/v1/state/stream` sends the current main state snapshot immediately after connection, then sends updates only when the main state content changes.
- `/api/v1/state/stream` and `/api/v1/main-state` are primarily display-state APIs and may include the current blocker's `interaction` snapshot. Real-time approval popups should still be triggered by `/api/v1/events/stream`.
- Status indicator plugins should not report events, write notification history, or write NiumaNotifier persistent files directly.
- Display should be based on `status`. Do not infer main state from plugin ID, raw tool logs, or `event_type`.
- Plugins can use `NIUMA_PLUGIN_DATA_DIR` to save local runtime state such as window position or display style.
- SSE keep-alive comment lines should be ignored, and plugins should reconnect automatically after disconnection.

## Notification Result Writeback

After a notification plugin sends a real event notification, it should write the send result back:

```http
POST /api/v1/plugins/notification-results
Content-Type: application/json
```

Request body:

```json
{
  "plugin_id": "niuma-plugin-webhook",
  "event_id": "event-1",
  "status": "sent",
  "title": "Approval required",
  "body": "Project: repo\nTool: Codex\nEvent: approval required\nContent: Bash: cargo test",
  "reason": "approval_requested",
  "error_message": null,
  "sent_at": "2026-06-18T12:00:03Z"
}
```

Success response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "saved": true,
    "record_id": "plugin_notification:niuma-plugin-webhook:event-1"
  }
}
```

Field reference:

| Field | Required | Description |
| --- | --- | --- |
| `plugin_id` | Yes | Notification plugin ID. The plugin must be `kind = notification`. |
| `event_id` | Yes | The notified `NiumaEvent.id`. |
| `status` | Yes | Currently supports only `sent` and `failed`. |
| `title` | No | Actual sent title. |
| `body` | No | Actual sent body. |
| `reason` | No | Send reason, for example `approval_requested`. |
| `error_message` | No | Send failure reason. |
| `sent_at` | No | Successful send time. If `status = sent` and this field is omitted, the main app uses the current time. |

Constraints:

- `event_id` must refer to an existing event.
- Non-notification plugins receive a business validation failure.
- `status = failed` does not save `sent_at`.
- The same `plugin_id + event_id` overwrites the same plugin notification record, which is suitable for retrying after failure and writing back the final result.

## Test Notification Result Writeback

After a notification plugin receives and processes a `notification_test` SSE event, it should write the test result back:

```http
POST /api/v1/plugins/notification-test-results
Content-Type: application/json
```

Request body:

```json
{
  "plugin_id": "niuma-plugin-webhook",
  "test_id": "manual-test:niuma-plugin-webhook:1",
  "status": "sent",
  "title": "Test notification",
  "body": "This is a test notification",
  "error_message": null,
  "sent_at": "2026-06-18T12:00:03Z"
}
```

Success response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "saved": true,
    "record_id": "plugin_notification_test:niuma-plugin-webhook:manual-test:niuma-plugin-webhook:1"
  }
}
```

## Plugin Management API

Plugin management APIs are primarily used by the main UI, but can also be used for local debugging.

### List Plugins

```http
GET /api/v1/plugins
```

Response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "list": [
      {
        "id": "builtin-codex",
        "kind": "tool",
        "tool_id": "codex",
        "display_name": "Codex",
        "version": "0.1.0",
        "source": "builtin",
        "enabled": true,
        "runtime_status": "running",
        "last_error": null,
        "icon_url": null,
        "capabilities": ["event_watcher"],
        "config_schema": [],
        "install_path": null
      }
    ]
  }
}
```

Supported `runtime_status` values:

```text
starting
stopped
stopping
running
failed
```

### Import External Plugin

```http
POST /api/v1/plugins/import
Content-Type: application/json
```

Request body:

```json
{
  "source_dir": "/path/to/niuma-plugin-example"
}
```

The main app copies the whole directory into the user plugin directory. The destination directory name is the manifest `id`.

### Remove External Plugin

```http
POST /api/v1/plugins/remove
Content-Type: application/json
```

Request body:

```json
{
  "plugin_id": "niuma-plugin-example"
}
```

Built-in plugins cannot be removed.

### Enable Or Disable Plugin

```http
POST /api/v1/plugins/enabled
Content-Type: application/json
```

Request body:

```json
{
  "plugin_id": "niuma-plugin-example",
  "enabled": true
}
```

Notes:

- `tool` plugins write to the tool listener configuration.
- Plugins without `tool_id` write to the general plugin enabled state.
- Enabled-state changes wake the plugin runtime manager.

### Read Plugin Configuration

```http
GET /api/v1/plugins/config?plugin_id=niuma-plugin-example
```

Response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "plugin_id": "niuma-plugin-example",
    "config": {
      "url": "https://example.com/webhook"
    },
    "config_schema": [
      {
        "key": "url",
        "type": "url",
        "label": "Webhook URL",
        "required": true,
        "default": null,
        "options": []
      }
    ]
  }
}
```

### Save Plugin Configuration

```http
POST /api/v1/plugins/config
Content-Type: application/json
```

Request body:

```json
{
  "plugin_id": "niuma-plugin-example",
  "config": {
    "url": "https://example.com/webhook",
    "token": "secret-token"
  }
}
```

After configuration is saved, the main app wakes the plugin runtime manager. Plugins should reconnect on configuration changes, or let the main app restart the process to refresh configuration.

## Debugging

When an external plugin fails to start, check these items first:

1. Whether `plugin.json` is valid JSON and `id` does not conflict with a built-in plugin.
2. Whether `platforms` contains the current platform, or is an empty array.
3. Whether `command` is executable, and whether a relative path is relative to the `plugin.json` directory.
4. Whether the plugin has been enabled in the UI or through `/api/v1/plugins/enabled`.
5. Whether `runtime_status` and `last_error` from `/api/v1/plugins` point to a startup error.
6. Whether the plugin can access `NIUMA_LOCAL_API_URL`.
7. Whether a notification plugin has stale or incorrect dedupe records in `NIUMA_PLUGIN_DATA_DIR`.

Useful runtime checks:

```bash
curl "$NIUMA_LOCAL_API_URL/api/v1/plugins"
curl "$NIUMA_LOCAL_API_URL/api/v1/main-state"
curl "$NIUMA_LOCAL_API_URL/api/v1/notification-records"
```

## SSE Display Boundary

Plugins are responsible only for producing or consuming `NiumaEvent`. State priority, completed-state retention time, blocker cleanup, and main state SSE publishing are all handled by the main app.

External status indicators should depend only on:

```text
GET /api/v1/state/stream
GET /api/v1/main-state
```

Do not infer main state from plugin ID, raw tool logs, or `event_type`. Approval popup triggers belong to `/api/v1/events/stream` and should be driven by `approval_requested`; display-state APIs are for recovery or rendering the current snapshot.

## Development Checklist

- `plugin.json` can be parsed as JSON, and `id` does not conflict with a built-in plugin.
- `platforms` contains the current platform, or is empty to support all platforms.
- `command` is executable in the plugin install directory, or the bare command can be found in the system `PATH`.
- Tool plugins declare `kind = tool`, `tool_id`, and `event_watcher`.
- Notification plugins declare `kind = notification` and `event_consumer`, and also declare `notification_test` if test notification is needed.
- Approval-capable consumers declare both `event_consumer` and `approval_handler`; `approval_handler` alone is not a valid runtime mode.
- Status indicator plugins declare `kind = status_indicator` and `state_consumer`.
- When a tool plugin reports events, `event.tool` exactly matches the manifest `tool_id`.
- Status indicator plugins only consume `/api/v1/state/stream` and do not infer main state themselves.
- `dedupe_key` is stable, so repeated scans do not create duplicate state.
- The plugin exits correctly on `SIGTERM` or the equivalent termination signal.
- The plugin uses `NIUMA_PARENT_PID` for parent-process self-cleanup.
- Notification plugins save send dedupe state in `NIUMA_PLUGIN_DATA_DIR`.
- External plugins read configuration through the Local API and do not depend on internal main app configuration file paths.
- Plugins do not directly read or write `niuma.sqlite`, and do not query historical events from the database.
- SSE reconnects automatically, and reconnecting is not treated as a reason to replay historical events.
- All JSON API calls check the outer `code` and `message`.
