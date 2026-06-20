# External SSE Integration Guide

This document is for integrations that need to read the local NiumaNotifier state from another system, such as a status bar, automation script, test console, desktop notification agent, or helper panel on a trusted local network.

## Purpose

NiumaNotifier is a local status notifier for AI coding tools. It watches hooks, session files, and runtime logs from tools such as Codex and Claude Code, normalizes raw tool events into `NiumaEvent`, and aggregates them into a stable main state.

External SSE integrations commonly use this stream to:

- Know whether an AI tool is currently running.
- Detect whether the user needs to handle approval, input, or an error.
- Trigger external automation, prompts, or notifications when a task completes.
- Create, observe, and reset local state flows in test environments.

By default, NiumaNotifier exposes the Local API only on localhost:

```text
http://127.0.0.1:27874
```

You can override the bind address with `NIUMA_LOCAL_API_ADDR`. LAN or external access is only possible when the service is explicitly bound to a non-loopback address.

## Integration Boundaries

- The Local API is designed for trusted local callers and does not include built-in authentication.
- JSON endpoints and SSE responses include CORS headers, so local browser pages can call them directly.
- If you bind the API to `0.0.0.0` or a LAN IP, add network-level access control outside NiumaNotifier.
- `NIUMA_DB_PATH` controls the SQLite notification history database used by the current instance; verify it when debugging notification history.
- When all AI listener switches are disabled, the public main state is forced to `idle`.

## Endpoint Overview

| Purpose | Method | Path | Response type | Stability |
| --- | --- | --- | --- | --- |
| Real-time main-state SSE | `GET` | `/api/v1/state/stream` | `text/event-stream` | Stable |
| Real-time event SSE | `GET` | `/api/v1/events/stream` | `text/event-stream` | Experimental |
| Query current main state | `GET` | `/api/v1/main-state` | JSON envelope | Stable |
| Reset local state | `POST` | `/api/v1/state/reset` | JSON envelope | Stable |

SSE is the streaming-protocol exception and does not use the unified JSON envelope. Regular HTTP JSON endpoints use:

```json
{
  "code": 0,
  "message": "ok",
  "data": {}
}
```

Common error codes:

| `code` | Meaning | Typical scenario |
| --- | --- | --- |
| `0` | Success | The request was handled successfully. |
| `100004` | Parameter format error | The JSON request body cannot be parsed. |
| `100101` | Business validation failed | The reset `confirm` value is incorrect. |
| `900001` | System error | Reading or calculating local state failed. |
| `900005` | Route not found | The request path is not registered. |

## Main-State SSE Stream

Request:

```http
GET /api/v1/state/stream
Accept: text/event-stream
```

After the connection is established, the server immediately sends one snapshot of the current main state. Later, it sends a new `state` event only when the state content changes. The server also performs a fallback refresh check every 5 seconds to cover completed-state expiry, cross-process writes, and missed runtime notifications.

Event format:

```text
event: state
id: 1
data: {"version":1,"status":"running","updated_at":"2026-06-13T12:00:00Z","session":{...},"detail":{...}}
```

Notes:

- `event` is always `state`.
- `id` is the same value as `data.version`; it represents the display version.
- `id` is not a resumable consumption offset. After reconnecting, accept the first event as a fresh snapshot, or call `/api/v1/main-state` for synchronization.
- The server may send SSE keep-alive comment lines. Clients should ignore non-`state` events.

## Event SSE Stream

Request:

```http
GET /api/v1/events/stream
Accept: text/event-stream
```

The event stream is intended for event consumer plugins. The server broadcasts only new `NiumaEvent`
items that were successfully stored and applied to the state machine. It does not replay historical
events and does not broadcast duplicate submissions that were deduplicated. Notification plugins decide
for themselves whether an event should trigger a push notification.

Event format:

```text
event: event
id: event-1
data: {"id":"event-1","tool":"codex","session_id":"s1","project_path":"/repo","project_name":"repo","event_type":"approval_requested","severity":"urgent","summary":"Bash: cargo test","created_at":"2026-06-19T12:00:00Z"}
```

## Main-State Fields

`data` is a `MainStatePayload`:

| Field | Type | Description |
| --- | --- | --- |
| `version` | number | SSE display version. It increments when the state content changes. Regular queries may return `0`. |
| `status` | string | Current main state. See "Status Semantics". |
| `updated_at` | string/null | Event time for the current main state, in ISO 8601 format. |
| `session` | object/null | Session associated with the current main state. Usually `null` for `idle`. |
| `detail` | object/null | Event detail associated with the current main state. Usually `null` for `idle`. |

`session`:

```json
{
  "id": "session-id",
  "tool": "codex",
  "project_name": "NiuMaNotifier",
  "project_path": "/path/to/project"
}
```

`detail`:

```json
{
  "event_id": "event-id",
  "event_type": "approval_requested",
  "severity": "urgent",
  "summary": "Bash: cargo test",
  "content": "Bash: cargo test",
  "error_message": null,
  "payload_ref": null,
  "completion_reason": null,
  "failure_reason": null
}
```

`detail` field reference:

| Field | Type | Description |
| --- | --- | --- |
| `event_id` | string | Associated `NiumaEvent` ID. |
| `event_type` | string | Raw event type name, such as `approval_requested` or `task_failed`. |
| `severity` | string | Display severity. Common values are `info`, `urgent`, and `error`. |
| `summary` | string | Short user-facing summary. |
| `content` | string/null | Displayable body or command content. |
| `error_message` | string/null | Error detail. Prefer this field when `status` is `error`. |
| `payload_ref` | string/null | Optional reference to a larger payload. |
| `completion_reason` | string/null | Completion reason. |
| `failure_reason` | string/null | Failure reason. |

External systems should use `status` directly to determine the main state. Do not infer the main state from `event_type`.

## Status Semantics

| Status | Meaning | Integration guidance |
| --- | --- | --- |
| `idle` | There is no current activity to display. Internal `stale` is also exposed as `idle`. | Treat as no active task or actionable item. |
| `running` | An AI tool task is running and has recent activity. | Show "running"; usually no user interruption is needed. |
| `waiting_approval` | The tool is waiting for user approval, such as command execution, privilege escalation, or external access. | Notify with high priority. |
| `waiting_input` | The tool is waiting for user input. | Prompt the user to return to the tool or main UI. |
| `completed` | A recent task has completed. This state is retained for 1 minute by default, then becomes `idle`. | Use for completion notifications or external automation. |
| `error` | A tool task failed or needs attention. | Notify with high priority and prefer `detail.error_message`. |

Main-state priority:

1. Earliest `waiting_approval` / `waiting_input`.
2. Earliest `error`.
3. Latest `running` / `completed` activity.
4. `idle` when there is no activity.

## Current-State Query

Request:

```http
GET /api/v1/main-state
```

Response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "state": {
      "version": 0,
      "status": "idle",
      "updated_at": null,
      "session": null,
      "detail": null
    }
  }
}
```

Notes:

- This endpoint is useful for initial synchronization, reconnect fallback, and debugging.
- Real-time integrations should primarily use `/api/v1/state/stream`.
- The `version` in regular queries may be `0`; the SSE `version` is the display version.

## Reset State Endpoint

Reset is the official recovery endpoint for restoring NiumaNotifier's local aggregated state to `idle` when the main state cannot recover by itself.

Request:

```http
POST /api/v1/state/reset
Content-Type: application/json
```

```json
{
  "confirm": "RESET_NIUMA_STATE",
  "reason": "state_stuck"
}
```

Fields:

| Field | Type | Required | Description |
| --- | --- | --- | --- |
| `confirm` | string | Yes | Must be `RESET_NIUMA_STATE`, used to prevent accidental calls. |
| `reason` | string | No | Caller-provided reason, such as `state_stuck` or `operator_request`. |

Business failure when `confirm` is incorrect:

```json
{
  "code": 100101,
  "message": "confirm 必须为 RESET_NIUMA_STATE",
  "data": null
}
```

Successful response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "reset": true,
    "reset_at": "2026-06-13T12:00:00Z",
    "event_count": 0,
    "session_count": 0,
    "state": {
      "version": 0,
      "status": "idle",
      "updated_at": null,
      "session": null,
      "detail": null
    }
  }
}
```

Important notes:

- This endpoint resets the in-memory aggregated state used by the current Local API instance.
- After a successful reset, the runtime event bus publishes a reset event, and connected SSE clients receive a new `state` event.
- Reset only restores NiumaNotifier's aggregated state. It does not stop or repair underlying tools such as Codex or Claude Code.
- If the underlying tool continues writing session/log events, the state may become `running`, `waiting_approval`, `waiting_input`, or `error` again after reset.
- Verify the target Local API address before calling reset.

## JavaScript Example

```ts
const apiUrl = 'http://127.0.0.1:27874'
const stream = new EventSource(`${apiUrl}/api/v1/state/stream`)

stream.addEventListener('state', (event) => {
  // SSE data is a bare MainStatePayload, not a JSON envelope.
  const state = JSON.parse(event.data)
  console.log(state.status, state.session, state.detail)
})

stream.onerror = () => {
  // Browser EventSource reconnects automatically; call /api/v1/main-state if strict sync is needed.
  console.warn('NiumaNotifier SSE disconnected, browser will retry automatically')
}
```

Node.js environments can use an EventSource-compatible library. After reconnecting, still treat the first `state` event as a complete snapshot.

## curl Debugging

Listen to SSE:

```bash
curl -N http://127.0.0.1:27874/api/v1/state/stream
```

Query current state:

```bash
curl -s http://127.0.0.1:27874/api/v1/main-state
```

Reset state:

```bash
curl -s -X POST http://127.0.0.1:27874/api/v1/state/reset \
  -H 'Content-Type: application/json' \
  -d '{"confirm":"RESET_NIUMA_STATE","reason":"state_stuck"}'
```

## Troubleshooting

| Symptom | Recommendation |
| --- | --- |
| Cannot connect to `/api/v1/state/stream` | Confirm that the NiumaNotifier Local API is running and check `NIUMA_LOCAL_API_ADDR`. |
| State is always `idle` | Confirm that AI listening is enabled and that the request is sent to the target Local API instance. |
| `completed` disappears quickly | This is expected. Completed state is retained for 1 minute by default. |
| State becomes running or waiting again after reset | The underlying tool is still writing new events; return to that tool and handle it there. |
| Browser CORS fails | Confirm that the request reaches the Local API directly and that no proxy or gateway strips CORS headers. |

## Compatibility Contract

- The SSE `event` name is always `state`.
- Before adding a new `status` value, update this document and `docs/architecture/main-state-contract.md`.
- External systems should ignore unknown fields so future extensions remain compatible.
- External systems should treat `session` and `detail` as nullable.
- External systems should not infer the main state from `event_type`; use `status` directly.
