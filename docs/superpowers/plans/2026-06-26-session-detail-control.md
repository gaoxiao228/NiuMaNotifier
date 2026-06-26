# Session Detail Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在会话列表页面的详情底部加入稳定控制区，根据 `session_detail.data.control` 启用发送指令，并根据会话列表展示状态控制中断按钮。

**Architecture:** 前端新增轻量 session workbench/control view 模块，控制能力只从 `session_detail.data.control` 与 `control.actions` 读取；中断可点击状态只从会话列表当前展示的 runtime 状态读取。API 层负责统一响应解析和 control POST，视图层负责禁用、隐藏、busy 和错误展示。

**Tech Stack:** Tauri/Vite TypeScript UI、Local API `fetch`、现有 `src/i18n.ts` 多语言文案、现有无框架 DOM render 测试、CSS。

---

## File Structure

- Modify `src/api.ts`
  - 增加 tool session 详情、control 类型。
  - 增加 `getSessionDetail()`、`sendSessionInstruction()`、`interruptSession()`。
  - 增加保留统一响应 `message` 的 POST helper。
- Create `src/sessionControl.ts`
  - 纯函数判断 `send_instruction` / `interrupt` action 是否可用。
  - 纯函数生成 control request payload。
- Create `src/sessionWorkbenchView.ts`
  - 渲染会话详情底部控制区。
  - 提供事件绑定入口，处理发送、中断、busy 和错误状态。
- Modify `src/i18n.ts`
  - 补齐控制区文案。
- Modify `src/styles.css`
  - 增加控制区、输入框、发送按钮、中断按钮、错误提示样式。
- Create `tests/sessionControl.test.ts`
  - 测试 action 解析、按钮可用性和 request payload。
- Create `tests/sessionWorkbenchView.test.ts`
  - 测试 control 区渲染、禁用、隐藏、错误和发送成功清空输入。
- Modify `package.json`
  - 增加 `test:session-control` 和 `test:session-workbench-view`，并接入 `npm test`。

Current code note: 当前 `src/dashboardLayout.ts` 与 `tests/mainDashboardLayout.test.ts` 明确要求主界面不渲染 `#session-list`。执行计划时不要把会话工作台塞回主状态面板；如果已有会话列表入口在未提交改动或外部客户端中，应把 `sessionWorkbenchView.ts` 接入那个入口。如果当前仓库仍没有入口，本计划先产出可测试的 view/control 模块，后续入口接入单独处理。

---

### Task 1: Control Capability Pure Functions

**Files:**
- Create: `src/sessionControl.ts`
- Test: `tests/sessionControl.test.ts`
- Modify: `package.json`

- [ ] **Step 1: Write the failing test**

Create `tests/sessionControl.test.ts`:

```ts
import {
  buildInterruptPayload,
  buildSendInstructionPayload,
  findControlAction,
  getSessionControlState,
  type SessionControl
} from '../src/sessionControl'

const control: SessionControl = {
  available: true,
  provider: 'niuma_codex',
  wrapper_session_id: 'niuma_codex_1',
  capabilities: ['send_instruction', 'interrupt'],
  actions: [
    {
      type: 'send_instruction',
      transport: 'local_api',
      endpoint: '/api/v1/session-control/send-instruction'
    },
    {
      type: 'interrupt',
      transport: 'local_api',
      endpoint: '/api/v1/session-control/interrupt'
    }
  ]
}

if (findControlAction(control, 'send_instruction')?.endpoint !== '/api/v1/session-control/send-instruction') {
  throw new Error('应找到 send_instruction local_api action')
}

const runningState = getSessionControlState(control, 'running')
if (!runningState.canSendInstruction) {
  throw new Error('支持 send_instruction 时发送应可用')
}
if (!runningState.showInterrupt || !runningState.canInterrupt) {
  throw new Error('running 且支持 interrupt 时中断应显示且可点击')
}

const idleState = getSessionControlState(control, 'idle')
if (!idleState.showInterrupt || idleState.canInterrupt) {
  throw new Error('非 running 时中断应显示但禁用')
}

const noInterruptState = getSessionControlState(
  { ...control, capabilities: ['send_instruction'], actions: control.actions.slice(0, 1) },
  'running'
)
if (noInterruptState.showInterrupt) {
  throw new Error('没有 interrupt 能力时中断按钮应隐藏')
}

const unavailableState = getSessionControlState(null, 'running')
if (unavailableState.canSendInstruction || unavailableState.showInterrupt) {
  throw new Error('没有 control 时只能显示禁用发送入口，不显示中断')
}

const sendPayload = buildSendInstructionPayload({
  tool: 'codex',
  sessionId: 'codex-session-1',
  wrapperSessionId: 'niuma_codex_1',
  content: '继续'
})
if (sendPayload.session_id !== 'codex-session-1' || sendPayload.content !== '继续') {
  throw new Error('发送 payload 应包含 session_id 和 content')
}

const interruptPayload = buildInterruptPayload({
  tool: 'codex',
  sessionId: 'codex-session-1',
  wrapperSessionId: 'niuma_codex_1'
})
if (interruptPayload.wrapper_session_id !== 'niuma_codex_1') {
  throw new Error('中断 payload 应包含 wrapper_session_id')
}
```

- [ ] **Step 2: Add test script**

Modify `package.json` scripts:

```json
"test": "npm run test:main-dashboard-layout && npm run test:dashboard-auto-return && npm run test:responsive-layout-css && npm run test:status-summary && npm run test:listener-toggle && npm run test:settings-view && npm run test:plugin-transition && npm run test:plugin-runtime-refresh && npm run test:plugin-snapshot && npm run test:notification-settings-layout && npm run test:notification-history && npm run test:event-center && npm run test:event-center-window && npm run test:event-center-runtime && npm run test:session-control",
"test:session-control": "tsc --target ES2022 --module commonjs --moduleResolution node --lib ES2022,DOM --skipLibCheck --strict --esModuleInterop --outDir /tmp/niuma-session-control-test tests/sessionControl.test.ts && node /tmp/niuma-session-control-test/tests/sessionControl.test.js"
```

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
npm run test:session-control
```

Expected: FAIL because `src/sessionControl.ts` does not exist.

- [ ] **Step 4: Implement pure functions**

Create `src/sessionControl.ts`:

```ts
export type SessionControlActionType = 'send_instruction' | 'interrupt'

export type SessionControlAction = {
  type: SessionControlActionType | string
  transport?: string | null
  endpoint?: string | null
  debug_command?: string | null
}

export type SessionControl = {
  available: boolean
  provider?: string | null
  wrapper_session_id?: string | null
  capabilities: string[]
  actions: SessionControlAction[]
}

export type SessionControlState = {
  canSendInstruction: boolean
  sendInstructionEndpoint: string | null
  showInterrupt: boolean
  canInterrupt: boolean
  interruptEndpoint: string | null
  disabledReason: 'control_unavailable' | 'send_instruction_unsupported' | null
}

export type SendInstructionPayload = {
  tool: string
  session_id: string
  wrapper_session_id: string
  content: string
}

export type InterruptPayload = {
  tool: string
  session_id: string
  wrapper_session_id: string
}

export function findControlAction(
  control: SessionControl | null | undefined,
  actionType: SessionControlActionType
) {
  return (
    control?.actions.find(
      (action) =>
        action.type === actionType &&
        action.transport === 'local_api' &&
        typeof action.endpoint === 'string' &&
        action.endpoint.length > 0
    ) ?? null
  )
}

export function getSessionControlState(
  control: SessionControl | null | undefined,
  listRuntimeStatus: string | null | undefined
): SessionControlState {
  const sendAction = findControlAction(control, 'send_instruction')
  const interruptAction = findControlAction(control, 'interrupt')
  const available = control?.available === true
  const supportsSend =
    available && control.capabilities.includes('send_instruction') && Boolean(sendAction)
  const supportsInterrupt =
    available && control.capabilities.includes('interrupt') && Boolean(interruptAction)

  return {
    canSendInstruction: supportsSend,
    sendInstructionEndpoint: sendAction?.endpoint ?? null,
    showInterrupt: supportsInterrupt,
    canInterrupt: supportsInterrupt && listRuntimeStatus === 'running',
    interruptEndpoint: interruptAction?.endpoint ?? null,
    disabledReason: supportsSend
      ? null
      : available
        ? 'send_instruction_unsupported'
        : 'control_unavailable'
  }
}

export function buildSendInstructionPayload(input: {
  tool: string
  sessionId: string
  wrapperSessionId: string
  content: string
}): SendInstructionPayload {
  return {
    tool: input.tool,
    session_id: input.sessionId,
    wrapper_session_id: input.wrapperSessionId,
    content: input.content
  }
}

export function buildInterruptPayload(input: {
  tool: string
  sessionId: string
  wrapperSessionId: string
}): InterruptPayload {
  return {
    tool: input.tool,
    session_id: input.sessionId,
    wrapper_session_id: input.wrapperSessionId
  }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run:

```bash
npm run test:session-control
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add package.json tests/sessionControl.test.ts src/sessionControl.ts
git commit -m "feat: 新增会话控制能力判断" -m "修改内容：新增 session control 纯函数和测试，支持解析 send_instruction、interrupt action 以及生成控制接口请求体。" -m "修改原因：会话详情控制区需要稳定地按 session_detail.data.control 判断控件可用性，避免按工具名称硬编码。"
```

---

### Task 2: Local API Control Calls

**Files:**
- Modify: `src/api.ts`
- Test: compile through `tests/sessionControl.test.ts`

- [ ] **Step 1: Add API types and helper signatures**

Modify `src/api.ts` imports:

```ts
import { invoke } from '@tauri-apps/api/core'
import type { InterruptPayload, SendInstructionPayload, SessionControl } from './sessionControl'
```

Add types after `RuntimeStateListPayload`:

```ts
export type ToolSessionMessage = {
  role: string
  content: string | null
  created_at?: string | null
}

export type ToolSessionDetail = {
  tool: string
  session_id: string
  project_name?: string | null
  project_path?: string | null
  status?: string | null
  runtime_status?: string | null
  updated_at?: string | null
  messages: ToolSessionMessage[]
  next_cursor?: string | null
  control?: SessionControl | null
}

export type SessionControlResult = {
  wrapper_session_id: string
  result?: unknown
}

export type SendInstructionResult = SessionControlResult & {
  sent: boolean
}

export type InterruptSessionResult = SessionControlResult & {
  interrupted: boolean
}
```

- [ ] **Step 2: Add response-preserving POST helper**

Add below `requestLocalApi`:

```ts
async function postLocalApi<T>(path: string, payload: unknown) {
  const apiUrl = await getLocalApiUrl()
  const response = await fetch(`${apiUrl}${path}`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(payload)
  })
  const body = (await response.json()) as ApiResponse<T>
  if (body.code !== 0) {
    throw new Error(body.message)
  }
  return body.data
}
```

- [ ] **Step 3: Add session detail and control API functions**

Add near other exported API functions:

```ts
export async function getSessionDetail(input: {
  tool: string
  sessionId: string
  limit?: number
  cursor?: string | null
}) {
  const params = new URLSearchParams({
    tool: input.tool,
    session_id: input.sessionId,
    limit: String(input.limit ?? 100)
  })
  if (input.cursor) {
    params.set('cursor', input.cursor)
  }
  return await requestLocalApi<ToolSessionDetail>(`/api/v1/session_detail?${params.toString()}`)
}

export async function sendSessionInstruction(endpoint: string, payload: SendInstructionPayload) {
  return await postLocalApi<SendInstructionResult>(endpoint, payload)
}

export async function interruptSession(endpoint: string, payload: InterruptPayload) {
  return await postLocalApi<InterruptSessionResult>(endpoint, payload)
}
```

- [ ] **Step 4: Run TypeScript build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add src/api.ts
git commit -m "feat: 新增会话控制接口调用" -m "修改内容：为 session_detail、send_instruction 和 interrupt 增加前端 API 类型与调用函数。" -m "修改原因：会话详情控制区需要通过最新 Local API control action endpoint 发起控制请求。"
```

---

### Task 3: Session Workbench Control View

**Files:**
- Create: `src/sessionWorkbenchView.ts`
- Test: `tests/sessionWorkbenchView.test.ts`
- Modify: `package.json`

- [ ] **Step 1: Write render and interaction tests**

Create `tests/sessionWorkbenchView.test.ts`:

```ts
import { renderSessionDetailControl, bindSessionDetailControl } from '../src/sessionWorkbenchView'
import type { ToolSessionDetail } from '../src/api'

const detail: ToolSessionDetail = {
  tool: 'codex',
  session_id: 'codex-session-1',
  messages: [],
  control: {
    available: true,
    provider: 'niuma_codex',
    wrapper_session_id: 'niuma_codex_1',
    capabilities: ['send_instruction', 'interrupt'],
    actions: [
      {
        type: 'send_instruction',
        transport: 'local_api',
        endpoint: '/api/v1/session-control/send-instruction'
      },
      {
        type: 'interrupt',
        transport: 'local_api',
        endpoint: '/api/v1/session-control/interrupt'
      }
    ]
  }
}

const enabledHtml = renderSessionDetailControl({
  detail,
  listRuntimeStatus: 'running',
  text: {
    placeholder: '发送指令',
    send: '发送',
    interrupt: '中断',
    unsupported: '当前会话不支持发送指令'
  },
  busy: false,
  error: null
})

if (!enabledHtml.includes('class="session-detail-control"')) {
  throw new Error('详情底部应始终渲染控制区')
}
if (!enabledHtml.includes('data-session-action="send"') || enabledHtml.includes('data-session-action="send" disabled')) {
  throw new Error('支持 send_instruction 时发送按钮应可用')
}
if (!enabledHtml.includes('data-session-action="interrupt"') || enabledHtml.includes('data-session-action="interrupt" disabled')) {
  throw new Error('running 且支持 interrupt 时中断按钮应可用')
}

const idleHtml = renderSessionDetailControl({
  detail,
  listRuntimeStatus: 'idle',
  text: {
    placeholder: '发送指令',
    send: '发送',
    interrupt: '中断',
    unsupported: '当前会话不支持发送指令'
  },
  busy: false,
  error: null
})
if (!idleHtml.includes('data-session-action="interrupt" disabled')) {
  throw new Error('非 running 时中断按钮应显示但禁用')
}

const noControlHtml = renderSessionDetailControl({
  detail: { ...detail, control: null },
  listRuntimeStatus: 'running',
  text: {
    placeholder: '发送指令',
    send: '发送',
    interrupt: '中断',
    unsupported: '当前会话不支持发送指令'
  },
  busy: false,
  error: null
})
if (!noControlHtml.includes('data-session-control-input disabled')) {
  throw new Error('没有 control 时输入框应禁用')
}
if (noControlHtml.includes('data-session-action="interrupt"')) {
  throw new Error('没有 interrupt 能力时中断按钮应隐藏')
}

const root = document.createElement('div')
root.innerHTML = enabledHtml
const calls: string[] = []
bindSessionDetailControl(root, {
  detail,
  listRuntimeStatus: 'running',
  sendInstruction: async (_endpoint, payload) => {
    calls.push(payload.content)
  },
  interruptSession: async () => {
    calls.push('interrupt')
  },
  rerender: () => undefined
})
const input = root.querySelector<HTMLInputElement>('[data-session-control-input]')!
input.value = '继续'
root.querySelector<HTMLButtonElement>('[data-session-action="send"]')!.click()
await new Promise((resolve) => setTimeout(resolve, 0))
if (calls[0] !== '继续') {
  throw new Error('点击发送应提交输入内容')
}
if (input.value !== '') {
  throw new Error('发送成功后应清空输入框')
}
```

- [ ] **Step 2: Add test script**

Modify `package.json` scripts:

```json
"test": "npm run test:main-dashboard-layout && npm run test:dashboard-auto-return && npm run test:responsive-layout-css && npm run test:status-summary && npm run test:listener-toggle && npm run test:settings-view && npm run test:plugin-transition && npm run test:plugin-runtime-refresh && npm run test:plugin-snapshot && npm run test:notification-settings-layout && npm run test:notification-history && npm run test:event-center && npm run test:event-center-window && npm run test:event-center-runtime && npm run test:session-control && npm run test:session-workbench-view",
"test:session-workbench-view": "tsc --target ES2022 --module commonjs --moduleResolution node --lib ES2022,DOM --skipLibCheck --strict --esModuleInterop --outDir /tmp/niuma-session-workbench-view-test tests/sessionWorkbenchView.test.ts && node /tmp/niuma-session-workbench-view-test/tests/sessionWorkbenchView.test.js"
```

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
npm run test:session-workbench-view
```

Expected: FAIL because `src/sessionWorkbenchView.ts` does not exist.

- [ ] **Step 4: Implement control view**

Create `src/sessionWorkbenchView.ts`:

```ts
import type {
  InterruptSessionResult,
  SendInstructionResult,
  ToolSessionDetail
} from './api'
import {
  buildInterruptPayload,
  buildSendInstructionPayload,
  getSessionControlState
} from './sessionControl'
import { escapeHtml } from './viewUtils'

export type SessionDetailControlText = {
  placeholder: string
  send: string
  interrupt: string
  unsupported: string
}

export type SessionDetailControlRenderOptions = {
  detail: ToolSessionDetail
  listRuntimeStatus: string | null
  text: SessionDetailControlText
  busy: boolean
  error: string | null
}

export function renderSessionDetailControl(options: SessionDetailControlRenderOptions) {
  const state = getSessionControlState(options.detail.control, options.listRuntimeStatus)
  const inputDisabled = options.busy || !state.canSendInstruction
  const sendDisabled = inputDisabled
  const interruptDisabled = options.busy || !state.canInterrupt
  const message = options.error || (state.canSendInstruction ? '' : options.text.unsupported)

  return `
    <section class="session-detail-control" aria-label="${escapeHtml(options.text.placeholder)}">
      <div class="session-detail-control-main">
        <textarea
          class="session-detail-control-input"
          data-session-control-input
          rows="3"
          placeholder="${escapeHtml(options.text.placeholder)}"
          ${inputDisabled ? 'disabled' : ''}
        ></textarea>
        <button
          class="session-detail-control-send"
          data-session-action="send"
          type="button"
          ${sendDisabled ? 'disabled' : ''}
        >${escapeHtml(options.text.send)}</button>
      </div>
      ${
        state.showInterrupt
          ? `<button
              class="session-detail-control-interrupt"
              data-session-action="interrupt"
              type="button"
              ${interruptDisabled ? 'disabled' : ''}
            >${escapeHtml(options.text.interrupt)}</button>`
          : ''
      }
      <p class="session-detail-control-message" data-session-control-message ${message ? '' : 'hidden'}>${escapeHtml(message)}</p>
    </section>
  `
}

export type BindSessionDetailControlOptions = {
  detail: ToolSessionDetail
  listRuntimeStatus: string | null
  sendInstruction: (
    endpoint: string,
    payload: ReturnType<typeof buildSendInstructionPayload>
  ) => Promise<SendInstructionResult | void>
  interruptSession: (
    endpoint: string,
    payload: ReturnType<typeof buildInterruptPayload>
  ) => Promise<InterruptSessionResult | void>
  rerender: (error: string | null, busy: boolean) => void
}

export function bindSessionDetailControl(
  root: ParentNode,
  options: BindSessionDetailControlOptions
) {
  const input = root.querySelector<HTMLTextAreaElement>('[data-session-control-input]')
  const sendButton = root.querySelector<HTMLButtonElement>('[data-session-action="send"]')
  const interruptButton = root.querySelector<HTMLButtonElement>('[data-session-action="interrupt"]')
  const state = getSessionControlState(options.detail.control, options.listRuntimeStatus)
  const wrapperSessionId = options.detail.control?.wrapper_session_id

  sendButton?.addEventListener('click', async () => {
    const content = input?.value.trim() ?? ''
    if (!content || !state.canSendInstruction || !state.sendInstructionEndpoint || !wrapperSessionId) {
      return
    }
    options.rerender(null, true)
    try {
      await options.sendInstruction(
        state.sendInstructionEndpoint,
        buildSendInstructionPayload({
          tool: options.detail.tool,
          sessionId: options.detail.session_id,
          wrapperSessionId,
          content
        })
      )
      if (input) {
        input.value = ''
      }
      options.rerender(null, false)
    } catch (error) {
      options.rerender(error instanceof Error ? error.message : String(error), false)
    }
  })

  interruptButton?.addEventListener('click', async () => {
    if (!state.canInterrupt || !state.interruptEndpoint || !wrapperSessionId) {
      return
    }
    options.rerender(null, true)
    try {
      await options.interruptSession(
        state.interruptEndpoint,
        buildInterruptPayload({
          tool: options.detail.tool,
          sessionId: options.detail.session_id,
          wrapperSessionId
        })
      )
      options.rerender(null, false)
    } catch (error) {
      options.rerender(error instanceof Error ? error.message : String(error), false)
    }
  })
}
```

- [ ] **Step 5: Run test to verify it passes**

Run:

```bash
npm run test:session-workbench-view
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add package.json tests/sessionWorkbenchView.test.ts src/sessionWorkbenchView.ts
git commit -m "feat: 新增会话详情控制区视图" -m "修改内容：新增会话详情底部控制区渲染与事件绑定，支持发送指令、中断按钮显示禁用和错误提示。" -m "修改原因：会话列表详情页需要根据 session_detail.data.control 提供稳定控制入口。"
```

---

### Task 4: I18n And CSS

**Files:**
- Modify: `src/i18n.ts`
- Modify: `src/styles.css`
- Test: `tests/sessionWorkbenchView.test.ts`

- [ ] **Step 1: Add i18n keys**

Modify the translation type in `src/i18n.ts` by adding:

```ts
sessionControlPlaceholder: string
sessionControlSend: string
sessionControlInterrupt: string
sessionControlUnsupported: string
sessionControlFailed: string
```

Add values for every language object:

```ts
sessionControlPlaceholder: '输入要发送给当前会话的指令',
sessionControlSend: '发送',
sessionControlInterrupt: '中断',
sessionControlUnsupported: '当前会话不支持发送指令',
sessionControlFailed: '控制请求失败'
```

Use equivalent existing language style for Traditional Chinese, English, Japanese, Korean, and German:

```ts
sessionControlPlaceholder: 'Enter an instruction for this session',
sessionControlSend: 'Send',
sessionControlInterrupt: 'Interrupt',
sessionControlUnsupported: 'This session does not support sending instructions',
sessionControlFailed: 'Control request failed'
```

- [ ] **Step 2: Add control CSS**

Append to `src/styles.css` near other session/detail styles or before responsive rules:

```css
.session-detail-control {
  border-top: 1px solid var(--border-subtle);
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 10px;
  padding: 12px;
}

.session-detail-control-main {
  display: grid;
  grid-template-columns: minmax(0, 1fr) auto;
  gap: 8px;
  min-width: 0;
}

.session-detail-control-input {
  min-height: 72px;
  resize: vertical;
}

.session-detail-control-send,
.session-detail-control-interrupt {
  align-self: end;
  white-space: nowrap;
}

.session-detail-control-interrupt {
  color: var(--danger-text);
}

.session-detail-control-message {
  color: var(--text-muted);
  font-size: 12px;
  grid-column: 1 / -1;
  margin: 0;
}

@media (max-width: 720px) {
  .session-detail-control,
  .session-detail-control-main {
    grid-template-columns: 1fr;
  }

  .session-detail-control-send,
  .session-detail-control-interrupt {
    justify-self: end;
  }
}
```

If `--border-subtle` or `--danger-text` does not exist in `src/styles.css`, use existing nearby variables or literal colors already used for warning/error buttons in the file. Do not introduce a new one-note palette.

- [ ] **Step 3: Run tests and build**

Run:

```bash
npm run test:session-workbench-view
npm run build
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add src/i18n.ts src/styles.css
git commit -m "feat: 补齐会话控制区文案和样式" -m "修改内容：为会话详情控制区新增多语言文案和响应式样式。" -m "修改原因：控制区需要在不同语言和窄屏布局下保持可读、可用。"
```

---

### Task 5: Integrate With Session List Page

**Files:**
- Modify the actual session list/detail page file after locating it with `rg "session_detail|session_project_groups|session-list|session workbench|会话工作台" src`.
- If no file exists in current checkout, create a follow-up integration task instead of modifying `src/dashboardLayout.ts`.
- Test: existing page render test or `tests/sessionWorkbenchView.test.ts`

- [ ] **Step 1: Locate the current page**

Run:

```bash
rg -n "session_detail|session_project_groups|session-list|session workbench|会话工作台" src tests
```

Expected in current checkout: no implementation file for the session list page. If a page file appears because local uncommitted work has added it, use that file as the integration target.

- [ ] **Step 2: Wire the control view into the detail render**

In the session detail render function, append:

```ts
const controlHtml = renderSessionDetailControl({
  detail: selectedDetail,
  listRuntimeStatus: selectedListItem.runtime_status ?? selectedListItem.status ?? null,
  text: {
    placeholder: t.sessionControlPlaceholder,
    send: t.sessionControlSend,
    interrupt: t.sessionControlInterrupt,
    unsupported: t.sessionControlUnsupported
  },
  busy: sessionControlBusy,
  error: sessionControlError
})
```

Place `${controlHtml}` at the bottom of the detail panel after the message list container. Keep the message list independently scrollable so the bottom control area does not overlap messages.

- [ ] **Step 3: Bind control actions after render**

After replacing detail panel HTML, call:

```ts
bindSessionDetailControl(detailPanelElement, {
  detail: selectedDetail,
  listRuntimeStatus: selectedListItem.runtime_status ?? selectedListItem.status ?? null,
  sendInstruction: sendSessionInstruction,
  interruptSession,
  rerender: (error, busy) => {
    sessionControlError = error
    sessionControlBusy = busy
    renderSelectedSessionDetail()
  }
})
```

- [ ] **Step 4: Preserve input on send failure**

If the page re-renders while a send request fails, store the current input value before re-render:

```ts
let sessionControlDraft = ''

const input = detailPanelElement.querySelector<HTMLTextAreaElement>('[data-session-control-input]')
sessionControlDraft = input?.value ?? sessionControlDraft
```

After render, restore:

```ts
const nextInput = detailPanelElement.querySelector<HTMLTextAreaElement>('[data-session-control-input]')
if (nextInput && sessionControlDraft) {
  nextInput.value = sessionControlDraft
}
```

Clear `sessionControlDraft = ''` only after `sendSessionInstruction` succeeds.

- [ ] **Step 5: Run focused tests**

Run:

```bash
npm run test:session-control
npm run test:session-workbench-view
```

Expected: PASS.

- [ ] **Step 6: Commit**

Run:

```bash
git add src tests package.json
git commit -m "feat: 接入会话详情底部控制区" -m "修改内容：将会话详情控制区接入会话列表详情页，按列表运行态控制中断按钮，并调用 send_instruction 和 interrupt 接口。" -m "修改原因：用户需要在会话列表详情底部直接发送指令，并在运行中会话上触发中断。"
```

---

### Task 6: Final Verification

**Files:**
- No code changes unless verification finds a bug.

- [ ] **Step 1: Run frontend tests**

Run:

```bash
npm test
```

Expected: PASS.

- [ ] **Step 2: Run frontend build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 3: Inspect git diff**

Run:

```bash
git status --short
git diff --stat
```

Expected: only intended files remain changed or no changes remain after commits. Existing unrelated user changes may still appear; do not revert them.

---

## Self-Review

- Spec coverage: The plan covers always-rendered control area, disabled send controls without `send_instruction`, hidden interrupt without capability, disabled interrupt outside `running`, endpoint lookup from `control.actions`, unified error message handling, busy state, successful send clearing input, failed send preserving input, i18n, CSS, and tests.
- Scope check: No backend API work is planned because the latest docs already define the endpoints and the task is a session list/detail UI change.
- Current-code conflict: The current checkout does not contain an active session list page in `src`; `tests/mainDashboardLayout.test.ts` explicitly rejects rendering `#session-list` in the main dashboard. Task 5 therefore requires locating the actual session list page in the current execution checkout and must not reintroduce it into `renderDashboardShell` unless the product decision changes.
- Placeholder scan: The plan does not use `TBD` or deferred implementation placeholders. The only conditional path is the explicit current-code conflict in Task 5.
- Type consistency: `SessionControl`, `ToolSessionDetail`, `sendSessionInstruction`, `interruptSession`, `renderSessionDetailControl`, and `bindSessionDetailControl` are consistently named across tasks.
