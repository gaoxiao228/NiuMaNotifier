# Remote Access Diagnostics Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one-click diagnostics to the external Web console and local NiumaNotifier remote-access settings so users can locate remote-access failures by layer.

**Architecture:** Use a shared diagnostic report shape on each side, but keep execution ownership separate. The external Web console actively verifies the browser-to-device chain through connection creation, Relay, WebRTC, RPC ping, and session group reads; the local desktop app verifies whether the device is ready to be connected without creating a remote connection.

**Tech Stack:** React 19, TypeScript, Vitest, Tauri v2 command bridge, Rust remote modules, existing Plain RPC and local session service/helper APIs.

---

## File Structure

- Create `remote-server/web/src/remote/diagnostics.ts`
  - Owns external Web diagnostic report types and pure helpers such as step creation and `overall` calculation.
- Modify `remote-server/web/src/remote/remoteDeviceSessionController.ts`
  - Adds `runDiagnostics()` and diagnostic report state, while reusing existing connection, Relay, WebRTC, Plain RPC, and session stream logic.
- Modify `remote-server/web/src/remote/deviceConsolePage.tsx`
  - Adds the one-click diagnostic button and renders the latest report above session groups.
- Modify `remote-server/web/src/i18n/messages.ts`
  - Adds diagnostic labels for all supported UI languages already present in the remote Web app.
- Modify `remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts`
  - Covers external diagnostic report behavior and degraded/failure overall rules.
- Modify `remote-server/web/src/__tests__/deviceConsolePage.test.tsx`
  - Covers the button, disabled state, and report rendering.
- Create `src/remoteDiagnostics.ts`
  - Owns desktop-side TypeScript diagnostic report types and rendering helpers for the settings page.
- Modify `src/api.ts`
  - Adds `runRemoteAccessDiagnostics()` Tauri command wrapper.
- Modify `src/settingsView.ts`
  - Adds the local diagnostic button and report rendering in the remote-access panel.
- Modify `src/main.ts`
  - Wires click handling, busy state, result state, and refresh interaction for local diagnostics.
- Modify `src/i18n.ts`
  - Adds local diagnostic labels for `zh-CN`, `zh-TW`, `en`, `ja`, `ko`, and `de`.
- Modify `tests/remoteSettingsView.test.ts`
  - Covers local diagnostic button/report rendering.
- Create `tests/remoteDiagnosticsRender.test.ts`
  - Covers pure desktop diagnostic report rendering helpers.
- Create `src-tauri/src/remote/diagnostics.rs`
  - Implements local diagnostic report generation.
- Modify `src-tauri/src/remote/mod.rs`
  - Exposes the diagnostics module.
- Modify `src-tauri/src/main.rs`
  - Registers the new `run_remote_access_diagnostics` command.
- Modify `src-tauri/src/remote/commands.rs`
  - Adds the command entry point or command helper, following existing remote command organization.
- Test with `cd remote-server/web && npm test`, root `npm test`, root `npm run build`, and `cd src-tauri && cargo test -p niuma-desktop remote::`.

## Task 1: External Web Diagnostic Types And Pure Helpers

**Files:**
- Create: `remote-server/web/src/remote/diagnostics.ts`
- Test: `remote-server/web/src/__tests__/remoteDiagnostics.test.ts`

- [ ] **Step 1: Write the failing test**

Create `remote-server/web/src/__tests__/remoteDiagnostics.test.ts`:

```ts
import {
  createDiagnosticStep,
  finishDiagnosticReport,
  startDiagnosticReport,
  type DiagnosticStep
} from '../remote/diagnostics.js'

describe('remote diagnostics helpers', () => {
  it('marks relay fallback as degraded instead of failed', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({
        key: 'webrtc_rpc_ping',
        title: 'WebRTC Ping',
        status: 'failed',
        severity: 'warning',
        message: 'WebRTC ping timeout'
      }),
      createDiagnosticStep({ key: 'session_project_groups', title: 'Sessions', status: 'passed' })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('degraded')
    expect(finished.summary).toBe('diagnostics_summary_relay_available_webrtc_failed')
  })

  it('marks session read failure as failed even when transports work', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({ key: 'webrtc_rpc_ping', title: 'WebRTC Ping', status: 'passed' }),
      createDiagnosticStep({
        key: 'session_project_groups',
        title: 'Sessions',
        status: 'failed',
        severity: 'error',
        message: 'remote_sessions_failed'
      })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('failed')
    expect(finished.summary).toBe('diagnostics_summary_session_failed')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server/web && npx vitest run src/__tests__/remoteDiagnostics.test.ts
```

Expected: FAIL because `../remote/diagnostics.js` does not exist.

- [ ] **Step 3: Add the diagnostic helper module**

Create `remote-server/web/src/remote/diagnostics.ts`:

```ts
export type DiagnosticStepStatus = 'passed' | 'failed' | 'skipped' | 'running'
export type DiagnosticSeverity = 'info' | 'warning' | 'error'
export type DiagnosticOverall = 'passed' | 'degraded' | 'failed'
export type DiagnosticScope = 'web_client' | 'local_agent'

export type DiagnosticStep = {
  key: string
  title: string
  status: DiagnosticStepStatus
  severity?: DiagnosticSeverity
  duration_ms?: number
  message?: string
  suggestion?: string
  detail?: unknown
}

export type DiagnosticReport = {
  scope: DiagnosticScope
  overall: DiagnosticOverall
  summary: string
  started_at: string
  finished_at?: string
  steps: DiagnosticStep[]
}

type StepInput = Omit<DiagnosticStep, 'status'> & {
  status?: DiagnosticStepStatus
}

export function createDiagnosticStep(input: StepInput): DiagnosticStep {
  return {
    ...input,
    status: input.status ?? 'running'
  }
}

export function startDiagnosticReport(scope: DiagnosticScope, now = new Date()): DiagnosticReport {
  return {
    scope,
    overall: 'degraded',
    summary: 'diagnostics_summary_running',
    started_at: now.toISOString(),
    steps: []
  }
}

function stepStatus(steps: DiagnosticStep[], key: string): DiagnosticStepStatus | undefined {
  return steps.find((step) => step.key === key)?.status
}

function calculateWebClientOverall(steps: DiagnosticStep[]): DiagnosticOverall {
  const sessionStatus = stepStatus(steps, 'session_project_groups')
  const relayStatus = stepStatus(steps, 'relay_rpc_ping')
  const webRtcStatus = stepStatus(steps, 'webrtc_rpc_ping')

  if (sessionStatus === 'failed') return 'failed'
  if (relayStatus === 'passed' && webRtcStatus === 'passed' && sessionStatus === 'passed') return 'passed'
  if (relayStatus === 'passed' && sessionStatus === 'passed') return 'degraded'
  return steps.some((step) => step.status === 'failed' && step.severity === 'error') ? 'failed' : 'degraded'
}

function calculateSummary(overall: DiagnosticOverall, steps: DiagnosticStep[]): string {
  if (stepStatus(steps, 'session_project_groups') === 'failed') return 'diagnostics_summary_session_failed'
  if (
    overall === 'degraded' &&
    stepStatus(steps, 'relay_rpc_ping') === 'passed' &&
    stepStatus(steps, 'webrtc_rpc_ping') === 'failed'
  ) {
    return 'diagnostics_summary_relay_available_webrtc_failed'
  }
  if (overall === 'passed') return 'diagnostics_summary_passed'
  if (overall === 'failed') return 'diagnostics_summary_failed'
  return 'diagnostics_summary_degraded'
}

export function finishDiagnosticReport(
  report: DiagnosticReport,
  steps: DiagnosticStep[],
  now = new Date()
): DiagnosticReport {
  const overall = report.scope === 'web_client' ? calculateWebClientOverall(steps) : 'degraded'
  return {
    ...report,
    overall,
    summary: calculateSummary(overall, steps),
    finished_at: now.toISOString(),
    steps
  }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run:

```bash
cd remote-server/web && npx vitest run src/__tests__/remoteDiagnostics.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add remote-server/web/src/remote/diagnostics.ts remote-server/web/src/__tests__/remoteDiagnostics.test.ts
git commit -m "feat: 新增外部客户端诊断报告模型" -m "修改内容：新增远程 Web 诊断报告类型、汇总规则和单元测试。" -m "修改原因：为一键诊断提供稳定的数据结构和结果判定基础。"
```

## Task 2: External Web Controller runDiagnostics

**Files:**
- Modify: `remote-server/web/src/remote/remoteDeviceSessionController.ts`
- Test: `remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts`

- [ ] **Step 1: Write the failing tests**

Append this helper and these tests to `remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts`. If the file already has a `latest` helper, replace that helper with this version so the diagnostic tests and existing tests share the same behavior:

```ts
function latest(harness: { snapshots: RemoteDeviceSessionSnapshot[] }) {
  const snapshot = harness.snapshots[harness.snapshots.length - 1]
  if (!snapshot) throw new Error('expected at least one controller snapshot')
  return snapshot
}
```

```ts
it('creates a connection when diagnostics run without an active session', async () => {
  const harness = createHarness({ online: true })

  await harness.controller.runDiagnostics()

  expect(harness.connectionsApi.create).toHaveBeenCalledTimes(1)
  expect(latest(harness).diagnosticReport?.steps.some((step) => step.key === 'connection_create')).toBe(true)
})

it('reports degraded when relay ping works and WebRTC ping fails', async () => {
  const harness = createHarness({ online: true })

  const diagnosticPromise = harness.controller.runDiagnostics()
  openAcceptedConnection(harness)
  openReadyRelay(harness.relayOptions)
  respondRelay(harness, 'rpc.ping', { pong: true })
  openWebRtc(harness)
  respondWebRtcError(harness, 'rpc.ping', { code: 'timeout', message: 'WebRTC ping timeout' })
  emitSessionGroup(harness, { list: [], page: 1, page_size: 20, total: 0 })

  await diagnosticPromise

  expect(latest(harness).diagnosticReport?.overall).toBe('degraded')
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server/web && npx vitest run src/__tests__/remoteDeviceSessionController.test.ts
```

Expected: FAIL because `runDiagnostics` and `diagnosticReport` do not exist.

- [ ] **Step 3: Extend controller types and snapshot**

Modify the imports and types in `remoteDeviceSessionController.ts`:

```ts
import {
  createDiagnosticStep,
  finishDiagnosticReport,
  startDiagnosticReport,
  type DiagnosticReport,
  type DiagnosticStep
} from './diagnostics.js'
```

Update `RemoteDeviceSessionSnapshot`:

```ts
export type RemoteDeviceSessionSnapshot = {
  connectionStatus: ConnectionStatus | 'idle'
  relayStatus: RemoteDeviceTransportStatus
  webRtcStatus: RemoteDeviceTransportStatus
  activeTransport: RemoteDeviceActiveTransport
  connectionId: string | null
  error: string | null
  pingResult: RpcResultState
  stateResult: RpcResultState
  sessionsResult: RpcResultState
  diagnostics: {
    relay: RpcResultState
    webrtc: RpcResultState
  }
  diagnosticReport: DiagnosticReport | null
  diagnosticRunning: boolean
}
```

Update `RemoteDeviceSessionController`:

```ts
export type RemoteDeviceSessionController = {
  connect(): Promise<void>
  runDiagnostics(): Promise<void>
  close(): void
  handleSignalMessage(message: unknown): void
}
```

Update `initialSnapshot()` to include:

```ts
diagnosticReport: null,
diagnosticRunning: false
```

Update `cloneSnapshot()` to clone these fields:

```ts
diagnosticReport: snapshot.diagnosticReport ? cloneSnapshotValue(snapshot.diagnosticReport) as DiagnosticReport : null,
diagnosticRunning: snapshot.diagnosticRunning
```

- [ ] **Step 4: Add controller helpers**

Inside `createRemoteDeviceSessionController`, add focused helpers:

```ts
function waitForResult(
  generation: number,
  read: () => RpcResultState,
  timeoutMs: number,
  failureMessage: string
): Promise<RpcResultState> {
  const started = Date.now()
  return new Promise((resolve) => {
    const timer = setInterval(() => {
      if (!isActive(generation)) {
        clearInterval(timer)
        resolve(errorResult('remote_rpc_failed'))
        return
      }
      const result = read()
      if (result.status === 'ready' || result.status === 'error') {
        clearInterval(timer)
        resolve(result)
        return
      }
      if (Date.now() - started >= timeoutMs) {
        clearInterval(timer)
        resolve(errorResult(failureMessage))
      }
    }, 50)
  })
}

function reportStep(key: string, title: string, status: DiagnosticStep['status'], started: number, message?: string): DiagnosticStep {
  return createDiagnosticStep({
    key,
    title,
    status,
    duration_ms: Date.now() - started,
    severity: status === 'failed' ? 'error' : 'info',
    message
  })
}
```

- [ ] **Step 5: Implement runDiagnostics minimally**

Add this method in the returned controller object before `close()`:

```ts
async runDiagnostics() {
  if (snapshot.diagnosticRunning || snapshot.connectionStatus === 'connecting') return
  const generationAtStart = activeGeneration
  const report = startDiagnosticReport('web_client')
  const steps: DiagnosticStep[] = []
  patchSnapshot(generationAtStart, { diagnosticRunning: true, diagnosticReport: report })

  try {
    const deviceStepStarted = Date.now()
    if (!options.device.online) {
      steps.push(reportStep('device_online', 'diagnostics_step_device_online', 'failed', deviceStepStarted, 'device_offline'))
      patchSnapshot(generationAtStart, {
        diagnosticReport: finishDiagnosticReport(report, steps),
        diagnosticRunning: false
      })
      return
    }
    steps.push(reportStep('device_online', 'diagnostics_step_device_online', 'passed', deviceStepStarted))

    const connectionStepStarted = Date.now()
    if (!snapshot.connectionId || snapshot.connectionStatus === 'error') {
      await this.connect()
    }
    const generation = activeGeneration
    const accepted = await waitForResult(
      generation,
      () => (snapshot.connectionStatus === 'accepted' ? readyResult(true) : snapshot.connectionStatus === 'error' ? errorResult(snapshot.error) : loadingResult()),
      rpcTimeoutMs,
      'diagnostics_connection_accept_timeout'
    )
    steps.push(reportStep('connection_create', 'diagnostics_step_connection_create', accepted.status === 'ready' ? 'passed' : 'failed', connectionStepStarted, accepted.status === 'error' ? String(accepted.value) : undefined))

    const relay = await waitForResult(generation, () => snapshot.diagnostics.relay, rpcTimeoutMs, 'diagnostics_relay_ping_timeout')
    steps.push(reportStep('relay_rpc_ping', 'diagnostics_step_relay_rpc_ping', relay.status === 'ready' ? 'passed' : 'failed', Date.now(), relay.status === 'error' ? String(relay.value) : undefined))

    const webrtc = await waitForResult(generation, () => snapshot.diagnostics.webrtc, webRtcProbeTimeoutMs, 'diagnostics_webrtc_ping_timeout')
    steps.push(createDiagnosticStep({
      key: 'webrtc_rpc_ping',
      title: 'diagnostics_step_webrtc_rpc_ping',
      status: webrtc.status === 'ready' ? 'passed' : 'failed',
      severity: webrtc.status === 'ready' ? 'info' : 'warning',
      message: webrtc.status === 'error' ? String(webrtc.value) : undefined
    }))

    const sessions = await waitForResult(generation, () => snapshot.sessionsResult, rpcTimeoutMs, 'remote_sessions_failed')
    steps.push(reportStep('session_project_groups', 'diagnostics_step_session_project_groups', sessions.status === 'ready' ? 'passed' : 'failed', Date.now(), sessions.status === 'error' ? String(sessions.value) : undefined))

    patchSnapshot(generation, {
      diagnosticReport: finishDiagnosticReport(report, steps),
      diagnosticRunning: false
    })
  } catch (error) {
    steps.push(createDiagnosticStep({
      key: 'diagnostics_unexpected_error',
      title: 'diagnostics_step_unexpected_error',
      status: 'failed',
      severity: 'error',
      message: errorText(error, 'remote_rpc_failed')
    }))
    patchSnapshot(activeGeneration, {
      diagnosticReport: finishDiagnosticReport(report, steps),
      diagnosticRunning: false
    })
  }
}
```

Keep comments short and only around timing/connection reuse where the flow is not obvious.

- [ ] **Step 6: Run controller tests**

Run:

```bash
cd remote-server/web && npx vitest run src/__tests__/remoteDeviceSessionController.test.ts src/__tests__/remoteDiagnostics.test.ts
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/web/src/remote/remoteDeviceSessionController.ts remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts
git commit -m "feat: 增加外部客户端远程链路诊断" -m "修改内容：为远程设备会话控制器新增 runDiagnostics 和诊断报告状态。" -m "修改原因：让外部 Web 控制台可以主动验证连接、Relay、WebRTC 和会话接口链路。"
```

## Task 3: External Web Diagnostic UI

**Files:**
- Modify: `remote-server/web/src/remote/deviceConsolePage.tsx`
- Modify: `remote-server/web/src/i18n/messages.ts`
- Modify: `remote-server/web/src/styles.css`
- Test: `remote-server/web/src/__tests__/deviceConsolePage.test.tsx`

- [ ] **Step 1: Write failing UI test**

Add to `deviceConsolePage.test.tsx`:

```tsx
it('renders one-click diagnostics and shows report rows', async () => {
  const harness = renderConsole({ online: true })

  await userEvent.click(screen.getByRole('button', { name: 'Run diagnostics' }))

  expect(screen.getByText('Diagnostics')).not.toBeNull()
  expect(screen.getByText('Device online')).not.toBeNull()
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server/web && npx vitest run src/__tests__/deviceConsolePage.test.tsx
```

Expected: FAIL because the button/report are not rendered.

- [ ] **Step 3: Render report component**

In `deviceConsolePage.tsx`, import an icon:

```ts
import { Activity, ArrowLeft, PlugZap, TerminalSquare } from 'lucide-react'
```

Add a small local renderer before `DeviceConsolePage`:

```tsx
function renderDiagnosticReport(t: (key: string) => string, report: RemoteDeviceSessionSnapshot['diagnosticReport']) {
  if (!report) return null
  return (
    <section className={`diagnostic-report diagnostic-report-${report.overall}`} aria-label={t('diagnostics')}>
      <div className="panel-title">{t('diagnostics')}</div>
      <p className="diagnostic-summary">{t(report.summary)}</p>
      <dl className="diagnostic-step-list">
        {report.steps.map((step) => (
          <div className="diagnostic-step" key={step.key}>
            <dt>{t(step.title)}</dt>
            <dd>
              <span className={`diagnostic-step-status diagnostic-step-status-${step.status}`}>
                {t(`diagnostic_status_${step.status}`)}
              </span>
              {typeof step.duration_ms === 'number' ? <span>{step.duration_ms}ms</span> : null}
              {step.message ? <span>{t(step.message)}</span> : null}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  )
}
```

In actions area add:

```tsx
<button
  type="button"
  className="secondary-button"
  onClick={() => void controller.runDiagnostics()}
  disabled={!device.online || snapshot.connectionStatus === 'connecting' || snapshot.diagnosticRunning}
>
  <Activity aria-hidden="true" size={16} />
  {snapshot.diagnosticRunning ? t('diagnostics_running') : t('run_diagnostics')}
</button>
```

Render report above sessions:

```tsx
{renderDiagnosticReport(t, snapshot.diagnosticReport)}
```

- [ ] **Step 4: Add translations**

In `messages.ts`, add these keys to every existing language map. Use the English values below for `en`, and translate the same meaning for the other maps:

```ts
run_diagnostics: 'Run diagnostics',
diagnostics_running: 'Diagnosing',
diagnostics: 'Diagnostics',
diagnostic_status_passed: 'Passed',
diagnostic_status_failed: 'Failed',
diagnostic_status_skipped: 'Skipped',
diagnostic_status_running: 'Running',
diagnostics_summary_running: 'Diagnostics running',
diagnostics_summary_passed: 'Remote access is available, using WebRTC when possible.',
diagnostics_summary_degraded: 'Remote access is available with degraded transport.',
diagnostics_summary_failed: 'Remote access is unavailable.',
diagnostics_summary_relay_available_webrtc_failed: 'Remote access is available through Relay, but WebRTC direct connection failed.',
diagnostics_summary_session_failed: 'Connection works, but remote sessions could not be loaded.',
diagnostics_step_device_online: 'Device online',
diagnostics_step_connection_create: 'Connection',
diagnostics_step_relay_rpc_ping: 'Relay RPC ping',
diagnostics_step_webrtc_rpc_ping: 'WebRTC RPC ping',
diagnostics_step_session_project_groups: 'Remote sessions',
diagnostics_step_unexpected_error: 'Unexpected error',
diagnostics_connection_accept_timeout: 'Timed out waiting for device response',
diagnostics_relay_ping_timeout: 'Relay ping timed out',
diagnostics_webrtc_ping_timeout: 'WebRTC ping timed out'
```

- [ ] **Step 5: Add CSS**

Append to `remote-server/web/src/styles.css`:

```css
.diagnostic-report {
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 12px;
  background: var(--panel-bg);
}

.diagnostic-summary {
  margin: 8px 0 12px;
  color: var(--text-muted);
}

.diagnostic-step-list {
  display: grid;
  gap: 8px;
  margin: 0;
}

.diagnostic-step {
  display: grid;
  grid-template-columns: minmax(140px, 1fr) 2fr;
  gap: 12px;
  align-items: center;
}

.diagnostic-step dt,
.diagnostic-step dd {
  margin: 0;
}

.diagnostic-step dd {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
  color: var(--text-muted);
}
```

- [ ] **Step 6: Run Web tests**

Run:

```bash
cd remote-server/web && npm test
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/web/src/remote/deviceConsolePage.tsx remote-server/web/src/i18n/messages.ts remote-server/web/src/styles.css remote-server/web/src/__tests__/deviceConsolePage.test.tsx
git commit -m "feat: 增加外部控制台诊断界面" -m "修改内容：在远程设备控制台增加一键诊断按钮和诊断报告展示。" -m "修改原因：让用户可以从外部 Web 侧直接查看远程链路健康情况。"
```

## Task 4: Desktop Local Diagnostic Command

**Files:**
- Create: `src-tauri/src/remote/diagnostics.rs`
- Modify: `src-tauri/src/remote/mod.rs`
- Modify: `src-tauri/src/remote/commands.rs`
- Modify: `src-tauri/src/main.rs`
- Test: Rust tests inside `src-tauri/src/remote/diagnostics.rs`

- [ ] **Step 1: Write failing Rust tests**

Create `src-tauri/src/remote/diagnostics.rs` with tests first:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use niuma_core::remote::agent_state::RemoteAgentState;
    use niuma_core::remote::config::{RemoteConfig, RemoteDeviceSummary, RemoteUserSummary};

    #[test]
    fn report_fails_when_credential_is_missing() {
        let mut config = RemoteConfig::default_for_server("http://127.0.0.1:27880");
        config.user = Some(RemoteUserSummary {
            id: "user_1".to_string(),
            email: "user@example.com".to_string(),
            role: "owner".to_string(),
        });
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "MacBook".to_string(),
        });

        let status = crate::remote::status::RemoteAgentStatus::new(RemoteAgentState::Online);
        let report = build_remote_access_diagnostics_report(config, false, status, Ok(3));

        assert_eq!(report.overall, DiagnosticOverall::Failed);
        assert!(report.steps.iter().any(|step| step.key == "credential_present" && step.status == DiagnosticStepStatus::Failed));
    }

    #[test]
    fn missing_active_connection_is_skipped_not_failed() {
        let mut config = RemoteConfig::default_for_server("http://127.0.0.1:27880");
        config.remote_access_enabled = true;
        config.remote_control_enabled = true;
        config.user = Some(RemoteUserSummary {
            id: "user_1".to_string(),
            email: "user@example.com".to_string(),
            role: "owner".to_string(),
        });
        config.device = Some(RemoteDeviceSummary {
            id: "dev_1".to_string(),
            name: "MacBook".to_string(),
        });

        let status = crate::remote::status::RemoteAgentStatus::new(RemoteAgentState::Online);
        let report = build_remote_access_diagnostics_report(config, true, status, Ok(3));

        assert_ne!(report.overall, DiagnosticOverall::Failed);
        assert!(report.steps.iter().any(|step| step.key == "active_connection" && step.status == DiagnosticStepStatus::Skipped));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd src-tauri && cargo test -p niuma-desktop remote::diagnostics::
```

Expected: FAIL because diagnostic types/functions are not implemented.

- [ ] **Step 3: Implement Rust diagnostic types and pure builder**

Replace the top of `diagnostics.rs` with:

```rust
use niuma_core::remote::config::RemoteConfig;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticStepStatus {
    Passed,
    Failed,
    Skipped,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticOverall {
    Passed,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticStep {
    pub key: &'static str,
    pub title: &'static str,
    pub status: DiagnosticStepStatus,
    pub severity: DiagnosticSeverity,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticReport {
    pub scope: &'static str,
    pub overall: DiagnosticOverall,
    pub summary: &'static str,
    pub steps: Vec<DiagnosticStep>,
}

fn step(
    key: &'static str,
    title: &'static str,
    status: DiagnosticStepStatus,
    severity: DiagnosticSeverity,
    message: Option<String>,
) -> DiagnosticStep {
    DiagnosticStep {
        key,
        title,
        status,
        severity,
        message,
    }
}

pub fn build_remote_access_diagnostics_report(
    config: RemoteConfig,
    has_credential: bool,
    status: crate::remote::status::RemoteAgentStatus,
    local_session_group_count: Result<usize, String>,
) -> DiagnosticReport {
    let mut steps = Vec::new();

    steps.push(step(
        "server_url",
        "remoteDiagnosticServerUrl",
        DiagnosticStepStatus::Passed,
        DiagnosticSeverity::Info,
        Some(config.server_url.clone()),
    ));
    steps.push(step(
        "remote_access_enabled",
        "remoteDiagnosticAccessEnabled",
        if config.remote_access_enabled { DiagnosticStepStatus::Passed } else { DiagnosticStepStatus::Failed },
        if config.remote_access_enabled { DiagnosticSeverity::Info } else { DiagnosticSeverity::Error },
        None,
    ));
    steps.push(step(
        "remote_control_enabled",
        "remoteDiagnosticControlEnabled",
        if config.remote_control_enabled { DiagnosticStepStatus::Passed } else { DiagnosticStepStatus::Failed },
        if config.remote_control_enabled { DiagnosticSeverity::Info } else { DiagnosticSeverity::Error },
        None,
    ));
    steps.push(step(
        "account_bound",
        "remoteDiagnosticAccountBound",
        if config.user.is_some() { DiagnosticStepStatus::Passed } else { DiagnosticStepStatus::Failed },
        if config.user.is_some() { DiagnosticSeverity::Info } else { DiagnosticSeverity::Error },
        None,
    ));
    steps.push(step(
        "device_bound",
        "remoteDiagnosticDeviceBound",
        if config.device.is_some() { DiagnosticStepStatus::Passed } else { DiagnosticStepStatus::Failed },
        if config.device.is_some() { DiagnosticSeverity::Info } else { DiagnosticSeverity::Error },
        None,
    ));
    steps.push(step(
        "credential_present",
        "remoteDiagnosticCredentialPresent",
        if has_credential { DiagnosticStepStatus::Passed } else { DiagnosticStepStatus::Failed },
        if has_credential { DiagnosticSeverity::Info } else { DiagnosticSeverity::Error },
        None,
    ));
    steps.push(step(
        "device_socket_status",
        "remoteDiagnosticDeviceSocketStatus",
        if status.state == "online" { DiagnosticStepStatus::Passed } else { DiagnosticStepStatus::Failed },
        if status.state == "online" { DiagnosticSeverity::Info } else { DiagnosticSeverity::Error },
        Some(status.state.to_string()),
    ));
    steps.push(step(
        "active_connection",
        "remoteDiagnosticActiveConnection",
        if status.active_connection_id.is_some() { DiagnosticStepStatus::Passed } else { DiagnosticStepStatus::Skipped },
        DiagnosticSeverity::Info,
        status.active_connection_id.or_else(|| Some("remoteNoActiveConnection".to_string())),
    ));
    steps.push(match local_session_group_count {
        Ok(count) => step(
            "local_session_project_groups",
            "remoteDiagnosticLocalSessions",
            DiagnosticStepStatus::Passed,
            DiagnosticSeverity::Info,
            Some(count.to_string()),
        ),
        Err(error) => step(
            "local_session_project_groups",
            "remoteDiagnosticLocalSessions",
            DiagnosticStepStatus::Failed,
            DiagnosticSeverity::Error,
            Some(error),
        ),
    });

    let has_error = steps
        .iter()
        .any(|item| item.status == DiagnosticStepStatus::Failed && item.severity == DiagnosticSeverity::Error);
    let overall = if has_error {
        DiagnosticOverall::Failed
    } else if steps.iter().any(|item| item.status == DiagnosticStepStatus::Skipped) {
        DiagnosticOverall::Degraded
    } else {
        DiagnosticOverall::Passed
    };
    let summary = match overall {
        DiagnosticOverall::Passed => "remoteDiagnosticSummaryPassed",
        DiagnosticOverall::Degraded => "remoteDiagnosticSummaryDegraded",
        DiagnosticOverall::Failed => "remoteDiagnosticSummaryFailed",
    };

    DiagnosticReport {
        scope: "local_agent",
        overall,
        summary,
        steps,
    }
}
```

- [ ] **Step 4: Wire module and command wrapper**

Modify `src-tauri/src/remote/mod.rs`:

```rust
pub mod diagnostics;
```

Add a command helper in `src-tauri/src/remote/commands.rs`:

```rust
pub fn run_remote_access_diagnostics_payload(
    config: RemoteConfig,
    has_credential: bool,
    status: crate::remote::status::RemoteAgentStatus,
    local_session_group_count: Result<usize, String>,
) -> serde_json::Value {
    serde_json::to_value(crate::remote::diagnostics::build_remote_access_diagnostics_report(
        config,
        has_credential,
        status,
        local_session_group_count,
    ))
    .unwrap_or_else(|_| serde_json::json!({ "scope": "local_agent", "overall": "failed", "summary": "remoteDiagnosticSummaryFailed", "steps": [] }))
}
```

Add the actual Tauri command to `src-tauri/src/commands.rs` next to `get_remote_agent_status`. It returns the same `ApiResponse` shape used by existing remote commands:

```rust
#[tauri::command]
pub(crate) fn run_remote_access_diagnostics(
    runtime_state: tauri::State<'_, AppRuntimeState>,
) -> ApiResponse<serde_json::Value> {
    let config = match runtime_state.store.remote_config() {
        Ok(value) => value,
        Err(error) => return ApiResponse::fail(ApiErrorCode::System, error),
    };
    let credential_store = crate::remote::commands::credential_store_for_data_dir(
        NiumaStore::default_path()
            .parent()
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(std::env::temp_dir),
    );
    let has_credential = credential_store.load(&config.server_url).is_ok();
    let status = runtime_state.remote_agent_status.snapshot();
    let local_session_group_count = Ok(0usize);
    ApiResponse::ok(crate::remote::commands::run_remote_access_diagnostics_payload(
        config,
        has_credential,
        status,
        local_session_group_count,
    ))
}
```

Modify `src-tauri/src/main.rs` and add the command to `tauri::generate_handler!` immediately after `commands::get_remote_agent_status`:

```rust
commands::run_remote_access_diagnostics,
```

- [ ] **Step 5: Run Rust tests**

Run:

```bash
cd src-tauri && cargo test -p niuma-desktop remote::diagnostics::
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/remote/diagnostics.rs src-tauri/src/remote/mod.rs src-tauri/src/remote/commands.rs src-tauri/src/main.rs
git commit -m "feat: 新增本机远程访问诊断命令" -m "修改内容：新增本机远程访问诊断报告模型、构建逻辑和 Tauri 命令入口。" -m "修改原因：让 NiumaNotifier 设置页可以检查本机是否具备被远程连接条件。"
```

## Task 5: Desktop Diagnostic API And Settings UI

**Files:**
- Create: `src/remoteDiagnostics.ts`
- Modify: `src/api.ts`
- Modify: `src/settingsView.ts`
- Modify: `src/main.ts`
- Modify: `src/i18n.ts`
- Modify: `src/styles.css`
- Test: `tests/remoteDiagnosticsRender.test.ts`
- Test: `tests/remoteSettingsView.test.ts`
- Modify: `package.json`

- [ ] **Step 1: Write failing render tests**

Create `tests/remoteDiagnosticsRender.test.ts`:

```ts
import { renderRemoteDiagnosticReport, type RemoteDiagnosticReport } from '../src/remoteDiagnostics'

const report: RemoteDiagnosticReport = {
  scope: 'local_agent',
  overall: 'degraded',
  summary: 'remoteDiagnosticSummaryDegraded',
  steps: [
    {
      key: 'active_connection',
      title: 'remoteDiagnosticActiveConnection',
      status: 'skipped',
      severity: 'info',
      message: 'remoteNoActiveConnection'
    }
  ]
}

if (!renderRemoteDiagnosticReport('zh-CN', report).includes('当前无外部客户端连接')) {
  throw new Error('本机诊断报告应渲染无外部客户端连接提示')
}
```

Add to `tests/remoteSettingsView.test.ts`:

```ts
assertIncludes(html, 'remote-diagnostics')
assertIncludes(html, translations['zh-CN'].remoteRunDiagnostics)
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
npm run test:remote-settings-view
```

Expected: FAIL because `remoteDiagnostics.ts` and UI labels do not exist.

- [ ] **Step 3: Add desktop diagnostic render helper**

Create `src/remoteDiagnostics.ts`:

```ts
import { translations, type LanguageCode } from './i18n'
import { escapeHtml } from './viewUtils'

export type RemoteDiagnosticStepStatus = 'passed' | 'failed' | 'skipped' | 'running'
export type RemoteDiagnosticOverall = 'passed' | 'degraded' | 'failed'

export type RemoteDiagnosticStep = {
  key: string
  title: string
  status: RemoteDiagnosticStepStatus
  severity?: 'info' | 'warning' | 'error'
  duration_ms?: number
  message?: string
}

export type RemoteDiagnosticReport = {
  scope: 'local_agent' | 'web_client'
  overall: RemoteDiagnosticOverall
  summary: string
  steps: RemoteDiagnosticStep[]
}

function translateMaybe(language: LanguageCode, value: string) {
  const t = translations[language] as unknown as Record<string, string>
  return t[value] ?? value
}

export function renderRemoteDiagnosticReport(language: LanguageCode, report: RemoteDiagnosticReport | null) {
  if (!report) return ''
  const t = translations[language] as unknown as Record<string, string>
  return `
    <section id="remote-diagnostics" class="remote-diagnostics remote-diagnostics-${escapeHtml(report.overall)}">
      <h3>${escapeHtml(t.remoteDiagnostics)}</h3>
      <p>${escapeHtml(translateMaybe(language, report.summary))}</p>
      <dl class="remote-diagnostic-steps">
        ${report.steps
          .map(
            (step) => `
              <div class="remote-diagnostic-step">
                <dt>${escapeHtml(translateMaybe(language, step.title))}</dt>
                <dd>
                  <span>${escapeHtml(t[`remoteDiagnosticStatus_${step.status}`] ?? step.status)}</span>
                  ${typeof step.duration_ms === 'number' ? `<span>${step.duration_ms}ms</span>` : ''}
                  ${step.message ? `<span>${escapeHtml(translateMaybe(language, step.message))}</span>` : ''}
                </dd>
              </div>
            `
          )
          .join('')}
      </dl>
    </section>
  `
}
```

- [ ] **Step 4: Add API wrapper**

In `src/api.ts`, add types import locally or export-compatible types:

```ts
import type { RemoteDiagnosticReport } from './remoteDiagnostics'
```

Add function:

```ts
export async function runRemoteAccessDiagnostics(): Promise<RemoteDiagnosticReport> {
  const response = await invoke<ApiResponse<RemoteDiagnosticReport>>('run_remote_access_diagnostics')
  return unwrapResponse(response)
}
```

Use the existing `ApiResponse` and `unwrapResponse` declarations already present in `src/api.ts`; do not add a second response wrapper.

- [ ] **Step 5: Wire settings render state**

Modify `RemoteSettingsRenderOptions` in `src/settingsView.ts`:

```ts
diagnosticReport?: RemoteDiagnosticReport | null
```

Import:

```ts
import { renderRemoteDiagnosticReport, type RemoteDiagnosticReport } from './remoteDiagnostics'
```

Add a button beside save:

```html
<button id="remote-diagnostics-run" type="button" ${options.busyAction === 'diagnostics' ? 'disabled' : ''}>
  ${escapeHtml(options.busyAction === 'diagnostics' ? t.remoteDiagnosticsRunning : t.remoteRunDiagnostics)}
</button>
```

Render report after summary:

```ts
${renderRemoteDiagnosticReport(options.language, options.diagnosticReport ?? null)}
```

- [ ] **Step 6: Wire main click handling**

In `src/main.ts`, import:

```ts
import { runRemoteAccessDiagnostics } from './api'
import type { RemoteDiagnosticReport } from './remoteDiagnostics'
```

Add state:

```ts
let remoteDiagnosticReport: RemoteDiagnosticReport | null = null
```

Pass it in `renderRemoteSettings()`:

```ts
diagnosticReport: remoteDiagnosticReport
```

Handle button click:

```ts
if (target?.id === 'remote-diagnostics-run') {
  remoteSettingsBusyAction = 'diagnostics'
  renderRemoteSettings()
  try {
    remoteDiagnosticReport = await runRemoteAccessDiagnostics()
    remoteSettingsResultText = ''
  } catch (error) {
    remoteSettingsResultText =
      error instanceof Error ? error.message : translations[currentLanguage].error
  } finally {
    remoteSettingsBusyAction = null
    renderRemoteSettings()
  }
  return
}
```

Extend `remoteSettingsBusyAction` union to include `'diagnostics'`.

- [ ] **Step 7: Add translations and CSS**

Add all new keys to every language block in `src/i18n.ts`:

```ts
remoteRunDiagnostics: string
remoteDiagnosticsRunning: string
remoteDiagnostics: string
remoteDiagnosticSummaryPassed: string
remoteDiagnosticSummaryDegraded: string
remoteDiagnosticSummaryFailed: string
remoteDiagnosticStatus_passed: string
remoteDiagnosticStatus_failed: string
remoteDiagnosticStatus_skipped: string
remoteDiagnosticStatus_running: string
remoteDiagnosticServerUrl: string
remoteDiagnosticAccessEnabled: string
remoteDiagnosticControlEnabled: string
remoteDiagnosticAccountBound: string
remoteDiagnosticDeviceBound: string
remoteDiagnosticCredentialPresent: string
remoteDiagnosticDeviceSocketStatus: string
remoteDiagnosticActiveConnection: string
remoteDiagnosticLocalSessions: string
```

Append CSS to `src/styles.css`:

```css
.remote-diagnostics {
  border: 1px solid var(--border-color);
  border-radius: 8px;
  padding: 12px;
}

.remote-diagnostics h3 {
  margin: 0 0 8px;
  font-size: 15px;
}

.remote-diagnostic-steps {
  display: grid;
  gap: 8px;
  margin: 12px 0 0;
}

.remote-diagnostic-step {
  display: grid;
  grid-template-columns: minmax(140px, 1fr) 2fr;
  gap: 12px;
}

.remote-diagnostic-step dt,
.remote-diagnostic-step dd {
  margin: 0;
}

.remote-diagnostic-step dd {
  display: flex;
  flex-wrap: wrap;
  gap: 8px;
}
```

- [ ] **Step 8: Add root test script**

In `package.json`, add:

```json
"test:remote-diagnostics-render": "tsc --target ES2022 --module commonjs --moduleResolution node --lib ES2022,DOM --skipLibCheck --strict --esModuleInterop --outDir /tmp/niuma-remote-diagnostics-render-test tests/remoteDiagnosticsRender.test.ts && node /tmp/niuma-remote-diagnostics-render-test/tests/remoteDiagnosticsRender.test.js"
```

Add it to the root `test` script near `test:remote-settings-view`.

- [ ] **Step 9: Run desktop tests**

Run:

```bash
npm run test:remote-diagnostics-render
npm run test:remote-settings-view
npm test
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add src/remoteDiagnostics.ts src/api.ts src/settingsView.ts src/main.ts src/i18n.ts src/styles.css tests/remoteDiagnosticsRender.test.ts tests/remoteSettingsView.test.ts package.json
git commit -m "feat: 增加本机远程访问诊断界面" -m "修改内容：在 NiumaNotifier 远程访问设置页新增一键诊断按钮、诊断报告渲染和前端命令调用。" -m "修改原因：让用户能从本机侧检查远程访问配置、绑定和设备状态。"
```

## Task 6: Full Verification And Manual Acceptance

**Files:**
- No new files unless verification exposes a concrete defect.

- [ ] **Step 1: Run full automated verification**

Run:

```bash
cd remote-server/web && npm test
npm test
npm run build
cd src-tauri && cargo test -p niuma-desktop remote::
```

Expected: all commands PASS.

- [ ] **Step 2: Run manual external Web acceptance**

Start the remote server and local app using the existing non-default project ports already configured in this branch. Then verify:

```text
1. Open external Web device console.
2. Click 一键诊断 without an existing connection.
3. Confirm a connection is created.
4. Confirm report includes device online, connection, Relay ping, WebRTC ping, and remote sessions.
5. Confirm Relay success + WebRTC failure shows degraded instead of failed when WebRTC is intentionally unavailable.
```

- [ ] **Step 3: Run manual local NiumaNotifier acceptance**

Verify:

```text
1. Open 设置 -> 远程访问.
2. Click 一键诊断.
3. Confirm report renders without creating a new external connection.
4. Confirm missing active connection is skipped/info, not failed.
5. Confirm missing credential or unbound account shows failed with clear message.
```

- [ ] **Step 4: Inspect and commit verification fixes**

Run:

```bash
git status --short
```

If verification produced no file changes, stop this task without a commit. If verification changed files, inspect the output and commit the changed implementation/test files shown by `git status --short`. For the common UI-test cleanup case, use:

```bash
git add remote-server/web/src/remote/deviceConsolePage.tsx src/settingsView.ts remote-server/web/src/__tests__/deviceConsolePage.test.tsx tests/remoteSettingsView.test.ts
git commit -m "fix: 修复远程访问诊断验收问题" -m "修改内容：修复自动化或手动验收发现的诊断显示/状态问题。" -m "修改原因：确保一键诊断结果稳定且符合设计文档。"
```

## Self-Review

- Spec coverage: external Web active diagnostics, local readiness diagnostics, shared report shape, UI display, error handling, tests, and non-goals are all mapped to tasks.
- Scope control: no history, export, auto-fix, device-list diagnostics, or local self-client simulation are included.
- API standard note: the new Tauri command returns the existing project `ApiResponse` shape. No new HTTP route is introduced, so the remote-server HTTP API standard is not broadened in this plan.
- Type consistency: `DiagnosticReport`, `DiagnosticStep`, `RemoteDiagnosticReport`, and `runDiagnostics()` names are consistent across tasks.
