# Managed niuma codex Sessions

`niuma codex` is the managed launcher for Codex CLI. It starts the real Codex process and adds a local relay plus a control socket so NiumaNotifier can identify the session and handle approvals, waiting input, resume instructions, and interrupts.

## Start Codex

In development, explicitly point to the real Codex binary:

```bash
NIUMA_REAL_CODEX="$(which codex)" ./target/debug/niuma codex
```

Arguments after `codex` are passed through to the real Codex CLI:

```bash
NIUMA_REAL_CODEX="$(which codex)" ./target/debug/niuma codex --model gpt-5.5
NIUMA_REAL_CODEX="$(which codex)" ./target/debug/niuma codex exec --help
```

After installing `niuma` into PATH:

```bash
niuma codex
```

## List Managed Sessions

```bash
./target/debug/niuma codex-sessions
```

Example response:

```json
{
  "code": 0,
  "message": "ok",
  "data": {
    "active_count": 1,
    "sessions": [
      {
        "wrapper_session_id": "niuma_codex_xxx",
        "state": "bound",
        "pid_alive": true,
        "control_socket_responsive": true,
        "codex_session_id": "codex-session-id",
        "control_socket": "/tmp/niuma-codex/xxx/control.sock"
      }
    ]
  }
}
```

`active_count = 0` means no currently controllable managed session was found. Common causes:

- Codex was started directly instead of through `niuma codex`.
- The `niuma` binary was not rebuilt and is still an older version.
- The Codex session exited and only a historical registry entry remains.
- The control socket is unavailable or unresponsive.

The registry file lives at:

```text
~/Library/Application Support/NiumaNotifier/managed-sessions/codex.json
```

It stores managed session indexes, binding data, and control socket paths. It is not user configuration.

## Waiting Input

When Codex app-server emits `requestUserInput`, the `niuma codex` relay submits an `input_requested` event. A Niuma-actionable input event includes:

```json
{
  "interaction": {
    "kind": "input",
    "handling": "niuma",
    "actionable": true,
    "request_id": "codex-input:niuma_codex_xxx:0",
    "endpoint": "/api/v1/session-control/answer-input",
    "schema": {
      "questions": [
        {
          "id": "app_type",
          "question": "Choose the app type",
          "options": [
            {
              "label": "Web App",
              "description": "Best for browser access"
            }
          ]
        }
      ]
    }
  }
}
```

The desktop UI renders controls from `interaction.schema.questions`. Submitting an answer calls:

```http
POST /api/v1/session-control/answer-input
```

`answers` uses `Record<string, string[]>`:

```json
{
  "tool": "codex",
  "session_id": "codex-session-id",
  "wrapper_session_id": "niuma_codex_xxx",
  "request_id": "codex-input:niuma_codex_xxx:0",
  "answers": {
    "app_type": ["Web App"]
  }
}
```

If the UI shows waiting input but no choices, first inspect the main-state interaction:

```bash
curl -s "http://127.0.0.1:27874/api/v1/main-state" | jq '.data.state.detail.interaction'
```

Then inspect pending relay requests:

```bash
CONTROL=$(./target/debug/niuma codex-sessions | jq -r '.data.sessions[0].control_socket')
printf '{"type":"requests"}\n' | nc -U "$CONTROL" | jq '.inputs'
```

Interpretation:

- `interaction.handling = "niuma"` and `actionable = true`: the UI should render a submit form.
- `interaction.handling = "tool"`: this is a watcher fallback event and must be handled in Codex.
- The control socket has inputs but main state has no relay event: check Local API availability and relay submissions to `/api/v1/plugin-events`.

## Approvals

Approvals continue to use:

```http
POST /api/v1/approval-decisions
```

When the relay observes an approval request, it submits a Niuma-actionable approval event through Local API. If the user accepts or rejects inside the Codex TUI, the relay syncs a “resolved in tool” state so stale UI buttons can be cleared.

## Send a Resume Instruction

Send a new instruction to a managed session:

```bash
./target/debug/niuma codex-send niuma_codex_xxx "Continue"
```

If the Codex thread is idle, the relay sends `turn/start`. If it is active, the relay sends `turn/steer`.

Matching Local API:

```http
POST /api/v1/session-control/send-instruction
```

## Interrupt

Interrupt the current turn in a managed session:

```bash
./target/debug/niuma codex-interrupt niuma_codex_xxx
```

Matching Local API:

```http
POST /api/v1/session-control/interrupt
```

Interrupt requires a current `inProgress` turn.

## Troubleshooting Checklist

1. Rebuild the CLI:

   ```bash
   cargo build -p niuma-cli
   ```

2. Check Local API:

   ```bash
   lsof -nP -iTCP:27874 -sTCP:LISTEN
   ```

3. Check whether an old Vite process owns the frontend port:

   ```bash
   lsof -nP -iTCP:58415 -sTCP:LISTEN
   ```

4. Confirm the session was started through the wrapper:

   ```bash
   ./target/debug/niuma codex-sessions
   ```

5. If the desktop UI does not refresh, restart `npm run tauri dev` or refresh the window.
