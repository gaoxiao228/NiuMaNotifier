# Remote Device List Session Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the remote web device list automatically verify the first online device, prove both relay and WebRTC business RPC are usable, and show the remote session list without entering the device console.

**Architecture:** Extract reusable remote session types/rendering and a shared remote device session controller from the current `DeviceConsolePage` connection logic. The controller owns signaling, relay, WebRTC, plain RPC, per-transport ping diagnostics, session stream subscription, fallback, and cleanup; `DeviceListPage` and `DeviceConsolePage` consume the same snapshot model.

**Tech Stack:** React, TypeScript, Vitest, Testing Library, existing remote WebSocket clients, existing plain RPC client, existing remote Local API stream bridge, Dockerized `remote-server`.

---

## File Structure

- Create `remote-server/web/src/remote/remoteSessionTypes.ts`
  - Owns `RemoteSessionProjectGroupPage`, group/session types, validation helpers, and display helper functions.
- Create `remote-server/web/src/remote/RemoteSessionGroupsView.tsx`
  - Renders session groups for both device list and console.
- Modify `remote-server/web/src/remote/remoteTransport.ts`
  - Adds explicit `sendVia(kind, value)` so diagnostics can force relay and WebRTC independently.
- Create `remote-server/web/src/remote/remoteDeviceSessionController.ts`
  - Owns the reusable connection lifecycle and exposes a snapshot callback.
- Modify `remote-server/web/src/devices/deviceListPage.tsx`
  - Auto-connects the first online device, shows channel diagnostics and remote sessions.
- Modify `remote-server/web/src/remote/deviceConsolePage.tsx`
  - Reuses shared session types/view; later step can migrate controller usage while preserving console debug panels.
- Modify `remote-server/web/src/App.tsx`
  - Passes `connectionsApi` into `DeviceListPage`.
- Modify `remote-server/web/src/i18n/messages.ts`
  - Adds translated labels for device-list remote session diagnostics.
- Test files:
  - `remote-server/web/src/__tests__/remoteTransport.test.ts`
  - `remote-server/web/src/__tests__/remoteSessionTypes.test.ts`
  - `remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts`
  - `remote-server/web/src/__tests__/deviceListPage.test.tsx`
  - Existing `remote-server/web/src/__tests__/deviceConsolePage.test.tsx`

---

### Task 1: Extract Session Types and Shared Session Renderer

**Files:**
- Create: `remote-server/web/src/remote/remoteSessionTypes.ts`
- Create: `remote-server/web/src/remote/RemoteSessionGroupsView.tsx`
- Modify: `remote-server/web/src/remote/deviceConsolePage.tsx`
- Test: `remote-server/web/src/__tests__/remoteSessionTypes.test.ts`
- Test: `remote-server/web/src/__tests__/deviceConsolePage.test.tsx`

- [ ] **Step 1: Write failing tests for session type validation**

Create `remote-server/web/src/__tests__/remoteSessionTypes.test.ts`:

```ts
import { describe, expect, it } from 'vitest'

import {
  isProjectGroupPage,
  sessionDescription,
  sessionDisplayStatus,
  sessionTitle
} from '../remote/remoteSessionTypes.js'

describe('remote session types', () => {
  it('accepts a valid session project group page', () => {
    expect(
      isProjectGroupPage({
        list: [
          {
            tool: 'codex',
            project_name: 'repo',
            project_path: '/repo',
            sessions: [
              {
                normalized_session_id: 'normalized-1',
                primary_session_id: 'primary-1',
                title: 'Demo session',
                runtime_status: 'running',
                status: 'idle',
                first_user_message_preview: 'Inspect work',
                latest_event_summary: null,
                subagent_count: 0
              }
            ]
          }
        ],
        page: 1,
        page_size: 20,
        total: 1
      })
    ).toBe(true)
  })

  it('rejects invalid session group shapes', () => {
    expect(isProjectGroupPage({ list: [{ sessions: 'not-array' }] })).toBe(false)
    expect(isProjectGroupPage({ list: [{ sessions: [{ subagent_count: '0' }] }] })).toBe(false)
  })

  it('derives display text from runtime status and fallbacks', () => {
    const session = {
      normalized_session_id: 'normalized-1',
      primary_session_id: 'primary-1',
      runtime_status: null,
      status: 'active',
      first_user_message_preview: '',
      latest_event_summary: 'Latest event'
    }
    expect(sessionDisplayStatus(session)).toBe('active')
    expect(sessionTitle(session)).toBe('primary-1')
    expect(sessionDescription(session)).toBe('Latest event')
  })
})
```

- [ ] **Step 2: Run the test to verify it fails**

Run:

```bash
cd remote-server/web
npm test -- remoteSessionTypes.test.ts
```

Expected: fails because `remoteSessionTypes.ts` does not exist.

- [ ] **Step 3: Create `remoteSessionTypes.ts`**

Create `remote-server/web/src/remote/remoteSessionTypes.ts`:

```ts
export type RemoteSessionProjectGroupPage = {
  list: RemoteSessionProjectGroup[]
  page?: number
  page_size?: number
  total?: number
}

export type RemoteSessionProjectGroup = {
  tool?: string
  project_name?: string
  project_path?: string
  sessions: RemoteSessionSummary[]
}

export type RemoteSessionSummary = {
  normalized_session_id?: string
  primary_session_id?: string
  title?: string
  status?: string
  runtime_status?: string | null
  updated_at?: string
  first_user_message_preview?: string
  latest_event_summary?: string | null
  subagent_count?: number
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export function isRemoteSessionSummary(value: unknown): value is RemoteSessionSummary {
  return (
    isRecord(value) &&
    (typeof value.normalized_session_id === 'undefined' || typeof value.normalized_session_id === 'string') &&
    (typeof value.primary_session_id === 'undefined' || typeof value.primary_session_id === 'string') &&
    (typeof value.title === 'undefined' || typeof value.title === 'string') &&
    (typeof value.status === 'undefined' || typeof value.status === 'string') &&
    (typeof value.runtime_status === 'undefined' ||
      value.runtime_status === null ||
      typeof value.runtime_status === 'string') &&
    (typeof value.updated_at === 'undefined' || typeof value.updated_at === 'string') &&
    (typeof value.first_user_message_preview === 'undefined' || typeof value.first_user_message_preview === 'string') &&
    (typeof value.latest_event_summary === 'undefined' ||
      value.latest_event_summary === null ||
      typeof value.latest_event_summary === 'string') &&
    (typeof value.subagent_count === 'undefined' || typeof value.subagent_count === 'number')
  )
}

export function isProjectGroupPage(value: unknown): value is RemoteSessionProjectGroupPage {
  return (
    isRecord(value) &&
    Array.isArray(value.list) &&
    value.list.every(
      (group) =>
        isRecord(group) &&
        (typeof group.tool === 'undefined' || typeof group.tool === 'string') &&
        (typeof group.project_name === 'undefined' || typeof group.project_name === 'string') &&
        (typeof group.project_path === 'undefined' || typeof group.project_path === 'string') &&
        Array.isArray(group.sessions) &&
        group.sessions.every(isRemoteSessionSummary)
    )
  )
}

export function sessionDisplayStatus(session: RemoteSessionSummary): string | null {
  return session.runtime_status || session.status || null
}

export function sessionTitle(session: RemoteSessionSummary): string {
  return session.title || session.primary_session_id || session.normalized_session_id || ''
}

export function sessionDescription(session: RemoteSessionSummary): string | null {
  return session.first_user_message_preview || session.latest_event_summary || session.primary_session_id || null
}
```

- [ ] **Step 4: Create the shared renderer**

Create `remote-server/web/src/remote/RemoteSessionGroupsView.tsx`:

```tsx
import {
  sessionDescription,
  sessionDisplayStatus,
  sessionTitle,
  type RemoteSessionProjectGroupPage
} from './remoteSessionTypes.js'

type RemoteSessionGroupsViewProps = {
  page: RemoteSessionProjectGroupPage
  emptyText: string
}

export function RemoteSessionGroupsView({ page, emptyText }: RemoteSessionGroupsViewProps) {
  const groups = page.list
  if (groups.length === 0 || groups.every((group) => group.sessions.length === 0)) {
    return <p className="state-message">{emptyText}</p>
  }

  return (
    <div className="remote-session-groups">
      {groups.map((group, groupIndex) => (
        <div className="remote-session-group" key={`${group.project_path ?? group.project_name ?? groupIndex}`}>
          <div className="remote-session-group-heading">
            {group.project_name ? <strong>{group.project_name}</strong> : null}
            {group.project_path ? <span>{group.project_path}</span> : null}
            {group.tool ? <span>{group.tool}</span> : null}
          </div>
          <div className="remote-session-list">
            {group.sessions.map((session, sessionIndex) => {
              const displayStatus = sessionDisplayStatus(session)
              const description = sessionDescription(session)
              return (
                <div
                  className="remote-session-row"
                  key={session.normalized_session_id ?? session.primary_session_id ?? `${groupIndex}-${sessionIndex}`}
                >
                  <div className="remote-session-main">
                    <strong>{sessionTitle(session)}</strong>
                    {description ? <span>{description}</span> : null}
                  </div>
                  {displayStatus ? <span className="remote-session-status">{displayStatus}</span> : null}
                </div>
              )
            })}
          </div>
        </div>
      ))}
    </div>
  )
}
```

- [ ] **Step 5: Modify `DeviceConsolePage` to use extracted types/view**

In `remote-server/web/src/remote/deviceConsolePage.tsx`:

Remove local definitions for `RemoteSessionProjectGroupPage`, `RemoteSessionProjectGroup`, `RemoteSessionSummary`, `isRecord`, `isRemoteSessionSummary`, `isProjectGroupPage`, `sessionDisplayStatus`, `sessionTitle`, `sessionDescription`.

Add imports:

```ts
import { RemoteSessionGroupsView } from './RemoteSessionGroupsView.js'
import { isProjectGroupPage } from './remoteSessionTypes.js'
```

Replace the ready branch inside `renderSessionGroups()` with:

```tsx
return <RemoteSessionGroupsView page={sessionsResult.value} emptyText={t('remote_sessions_empty')} />
```

- [ ] **Step 6: Verify tests**

Run:

```bash
cd remote-server/web
npm test -- remoteSessionTypes.test.ts deviceConsolePage.test.tsx
```

Expected: all selected tests pass.

- [ ] **Step 7: Commit**

```bash
git add remote-server/web/src/remote/remoteSessionTypes.ts \
  remote-server/web/src/remote/RemoteSessionGroupsView.tsx \
  remote-server/web/src/remote/deviceConsolePage.tsx \
  remote-server/web/src/__tests__/remoteSessionTypes.test.ts
git commit -m "refactor: 抽取远程会话类型和列表渲染" \
  -m "修改内容：将远程 session 类型校验和列表渲染从控制台页抽为共享模块。" \
  -m "修改原因：设备列表页和控制台页需要复用同一套 session 展示逻辑，避免后续重复实现。"
```

---

### Task 2: Add Explicit Transport Sending for Diagnostics

**Files:**
- Modify: `remote-server/web/src/remote/remoteTransport.ts`
- Test: `remote-server/web/src/__tests__/remoteTransport.test.ts`

- [ ] **Step 1: Add failing test for forced transport send**

Append to `remote-server/web/src/__tests__/remoteTransport.test.ts`:

```ts
it('sends through a specific open transport for diagnostics', () => {
  const relaySend = vi.fn()
  const webrtcSend = vi.fn()
  const bus = createRemoteMessageBus({ onInbound: vi.fn() })

  bus.register({ kind: 'relay', send: relaySend, close: vi.fn() })
  bus.register({ kind: 'webrtc', send: webrtcSend, close: vi.fn() })
  bus.setOpen('relay', true)
  bus.setOpen('webrtc', true)

  const selected = bus.sendVia('relay', {
    version: 1,
    type: 'request',
    id: 'rpc_1',
    method: 'rpc.ping',
    params: {},
    transport: { kind: 'webrtc' }
  })

  expect(selected).toBe('relay')
  expect(relaySend).toHaveBeenCalledWith(
    expect.objectContaining({
      method: 'rpc.ping',
      transport: { kind: 'relay' }
    })
  )
  expect(webrtcSend).not.toHaveBeenCalled()
})

it('throws when the requested transport is not open', () => {
  const bus = createRemoteMessageBus({ onInbound: vi.fn() })
  bus.register({ kind: 'relay', send: vi.fn(), close: vi.fn() })

  expect(() => bus.sendVia('relay', { transport: { kind: 'relay' } })).toThrow('Remote transport is not open: relay')
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server/web
npm test -- remoteTransport.test.ts
```

Expected: fails because `sendVia` does not exist.

- [ ] **Step 3: Implement `sendVia`**

Modify `RemoteMessageBus` type in `remote-server/web/src/remote/remoteTransport.ts`:

```ts
send(value: unknown): RemoteTransportKind
sendVia(kind: RemoteTransportKind, value: unknown): RemoteTransportKind
```

Add helper:

```ts
function findTransport(kind: RemoteTransportKind): RemoteTransport | null {
  const slot = transports.get(kind)
  return slot?.open ? slot.transport : null
}
```

Add implementation:

```ts
sendVia(kind, value) {
  const transport = findTransport(kind)
  if (!transport) throw new Error(`Remote transport is not open: ${kind}`)
  transport.send(markPayloadTransport(value, transport.kind))
  return transport.kind
}
```

Keep existing `send(value)` behavior unchanged.

- [ ] **Step 4: Verify selected tests**

Run:

```bash
cd remote-server/web
npm test -- remoteTransport.test.ts deviceConsolePage.test.tsx
```

Expected: tests pass.

- [ ] **Step 5: Commit**

```bash
git add remote-server/web/src/remote/remoteTransport.ts remote-server/web/src/__tests__/remoteTransport.test.ts
git commit -m "feat: 支持指定远程传输通道发送 RPC" \
  -m "修改内容：为远程消息总线增加 sendVia，用于强制通过 relay 或 WebRTC 发送诊断 RPC。" \
  -m "修改原因：设备列表页需要分别验证两个通道的业务 RPC 可用性，不能只依赖默认优先级路由。"
```

---

### Task 3: Create Shared Remote Device Session Controller

**Files:**
- Create: `remote-server/web/src/remote/remoteDeviceSessionController.ts`
- Test: `remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts`

- [ ] **Step 1: Write failing controller test for relay and WebRTC diagnostics**

Create `remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'

import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import { createRemoteDeviceSessionController } from '../remote/remoteDeviceSessionController.js'
import type { ConnectionClientOptions, ConnectionStatus } from '../remote/connectionClient.js'
import type { RelayClient, RelayClientOptions } from '../remote/relayTransport.js'
import type { WebRtcTransport, WebRtcTransportOptions } from '../remote/webrtcTransport.js'

function createConnectionResult(): ConnectionCreateResult {
  return {
    connection_id: 'conn_1',
    connection_token: 'token_1',
    expires_at: '2026-06-30T00:00:00.000Z',
    expires_in: 120,
    signaling_url: 'ws://127.0.0.1:27880/ws/client',
    relay_url: 'ws://127.0.0.1:27880/ws/relay'
  }
}

describe('remote device session controller', () => {
  it('checks relay and WebRTC RPC independently and streams sessions', async () => {
    const snapshots: any[] = []
    const connection = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      connection.onStatus = options.onStatus
      connection.onMessage = options.onMessage
      return connection
    })

    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })

    let webRtcOptions: WebRtcTransportOptions | null = null
    const webRtcSend = vi.fn()
    const createWebRtc = vi.fn((options: WebRtcTransportOptions): WebRtcTransport => {
      webRtcOptions = options
      return {
        kind: 'webrtc',
        start: vi.fn(async () => {}),
        acceptAnswer: vi.fn(),
        addRemoteIceCandidate: vi.fn(),
        send: webRtcSend,
        close: vi.fn()
      }
    })

    const controller = createRemoteDeviceSessionController({
      deviceId: 'dev_1',
      clientId: 'web_1',
      connectionsApi: { create: vi.fn().mockResolvedValue(createConnectionResult()) },
      createConnection,
      createRelay,
      createWebRtc,
      onSnapshot: (snapshot) => snapshots.push(snapshot)
    })

    await controller.start()
    connection.onStatus('accepted')
    relayOptions?.onOpen()
    relayOptions?.onReady()

    expect(relayClient.send).toHaveBeenCalledWith(expect.objectContaining({ method: 'rpc.ping' }))
    relayOptions?.onPayload({ version: 1, type: 'response', id: 'rpc_1', ok: true, result: { pong: true } })

    webRtcOptions?.onOpen()
    const probeRequest = webRtcSend.mock.calls[0]?.[0]
    webRtcOptions?.onPayload({
      version: 1,
      type: 'response',
      id: probeRequest.id,
      ok: true,
      result: { pong: true },
      transport: { kind: 'webrtc' }
    })

    const last = snapshots.at(-1)
    expect(last.relay.rpc).toBe('ok')
    expect(last.webrtc.rpc).toBe('ok')

    controller.close()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server/web
npm test -- remoteDeviceSessionController.test.ts
```

Expected: fails because `remoteDeviceSessionController.ts` does not exist.

- [ ] **Step 3: Implement controller types and snapshot defaults**

Create `remote-server/web/src/remote/remoteDeviceSessionController.ts` with exports:

```ts
import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteTransportKind } from './plainRpcClient.js'
import type { RemoteSessionProjectGroupPage } from './remoteSessionTypes.js'

export type ChannelSocketStatus = 'idle' | 'connecting' | 'open' | 'closed' | 'error'
export type ChannelRpcStatus = 'idle' | 'checking' | 'ok' | 'timeout' | 'error'

export type RemoteDeviceSessionSnapshot = {
  connectionStatus: 'idle' | 'connecting' | 'accepted' | 'rejected' | 'closed' | 'error'
  connectionId: string | null
  relay: { socket: ChannelSocketStatus; rpc: ChannelRpcStatus; error?: string }
  webrtc: { socket: ChannelSocketStatus; rpc: ChannelRpcStatus; error?: string }
  activeTransport: 'idle' | RemoteTransportKind
  sessions: {
    status: 'idle' | 'loading' | 'ready' | 'empty' | 'error'
    value: RemoteSessionProjectGroupPage | null
    error?: string
    transport?: RemoteTransportKind
  }
}

export const initialRemoteDeviceSessionSnapshot: RemoteDeviceSessionSnapshot = {
  connectionStatus: 'idle',
  connectionId: null,
  relay: { socket: 'idle', rpc: 'idle' },
  webrtc: { socket: 'idle', rpc: 'idle' },
  activeTransport: 'idle',
  sessions: { status: 'idle', value: null }
}
```

- [ ] **Step 4: Implement lifecycle by moving logic from `DeviceConsolePage`**

The controller should:

```ts
export function createRemoteDeviceSessionController(options: RemoteDeviceSessionControllerOptions) {
  let snapshot = initialRemoteDeviceSessionSnapshot
  let closed = false

  function patch(next: Partial<RemoteDeviceSessionSnapshot>) {
    snapshot = { ...snapshot, ...next }
    options.onSnapshot(snapshot)
  }

  async function start() {
    patch({ connectionStatus: 'connecting' })
    const result = await options.connectionsApi.create(options.deviceId, options.clientId)
    patch({ connectionId: result.connection_id })
    openSignalingAndTransports(result)
  }

  function close() {
    closed = true
    // close socket, relay, webrtc, rpc, stream
  }

  return { start, close, snapshot: () => snapshot }
}
```

Move the following existing behaviors from `DeviceConsolePage` into this controller:

- `buildClientSocketUrl`
- `createConnection`
- `buildRelaySocketUrl`
- `createRelay`
- `createWebRtc`
- `createRemoteMessageBus`
- `createPlainRpcClient`
- `createRemoteLocalApiClient`
- `readWebRtcAnswerSignal`
- `readWebRtcIceCandidateSignal`
- WebRTC probe before marking WebRTC usable
- relay `onReady` before marking relay usable
- session stream subscription path `/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20`

Diagnostics must use explicit transport sending:

```ts
function requestVia(kind: RemoteTransportKind, method: string, params: unknown = {}) {
  return rpcClient.requestWithSendOverride(method, params, (request) => messageBus.sendVia(kind, request))
}
```

If adding `requestWithSendOverride` to `PlainRpcClient` is too invasive, add a small controller-local diagnostic request map with ids `diag_relay_1` and `diag_webrtc_1`, send them with `messageBus.sendVia`, and resolve them from `messageBus.receive`.

- [ ] **Step 5: Add session stream fallback behavior**

When session stream creation or first event times out:

```ts
if (snapshot.webrtc.rpc === 'ok' && snapshot.relay.rpc === 'ok') {
  // Retry once through relay when WebRTC was active.
}
```

Use a 10 second timeout matching existing plain RPC timeout.

- [ ] **Step 6: Verify controller tests**

Run:

```bash
cd remote-server/web
npm test -- remoteDeviceSessionController.test.ts
```

Expected: controller tests pass.

- [ ] **Step 7: Commit**

```bash
git add remote-server/web/src/remote/remoteDeviceSessionController.ts \
  remote-server/web/src/__tests__/remoteDeviceSessionController.test.ts
git commit -m "feat: 新增远程设备会话控制器" \
  -m "修改内容：抽取远程连接、通道诊断和会话读取状态机，提供设备列表和控制台可复用的快照接口。" \
  -m "修改原因：需要用同一套逻辑证明 relay、WebRTC 和 session stream 都可用，避免两个页面各自维护连接状态。"
```

---

### Task 4: Show Remote Sessions on the Device List Page

**Files:**
- Modify: `remote-server/web/src/devices/deviceListPage.tsx`
- Modify: `remote-server/web/src/App.tsx`
- Modify: `remote-server/web/src/i18n/messages.ts`
- Test: `remote-server/web/src/__tests__/deviceListPage.test.tsx`

- [ ] **Step 1: Add failing device-list test**

Append to `remote-server/web/src/__tests__/deviceListPage.test.tsx`:

```tsx
import type { RemoteDeviceSessionSnapshot } from '../remote/remoteDeviceSessionController.js'

function readySnapshot(): RemoteDeviceSessionSnapshot {
  return {
    connectionStatus: 'accepted',
    connectionId: 'conn_1',
    relay: { socket: 'open', rpc: 'ok' },
    webrtc: { socket: 'open', rpc: 'ok' },
    activeTransport: 'webrtc',
    sessions: {
      status: 'ready',
      transport: 'webrtc',
      value: {
        list: [
          {
            tool: 'codex',
            project_name: 'repo',
            project_path: '/repo',
            sessions: [
              {
                normalized_session_id: 'session-1',
                title: 'Demo session',
                runtime_status: 'running',
                first_user_message_preview: 'Inspect remote sessions'
              }
            ]
          }
        ],
        page: 1,
        page_size: 20,
        total: 1
      }
    }
  }
}

it('shows remote sessions and both channel diagnostics for the first online device', async () => {
  const createRemoteSession = vi.fn((options: any) => {
    options.onSnapshot(readySnapshot())
    return { start: vi.fn(async () => {}), close: vi.fn(), snapshot: readySnapshot }
  })

  render(
    <DeviceListPage
      devicesApi={{
        async list() {
          return {
            list: [
              {
                id: 'dev_1',
                name: 'Desk Mac',
                online: true,
                last_seen_at: null,
                capabilities: {},
                identity_public_key: {}
              }
            ]
          }
        }
      }}
      connectionsApi={{ create: vi.fn() }}
      createRemoteSession={createRemoteSession}
      t={t}
      onSelectDevice={() => {}}
    />
  )

  expect(await screen.findByText('Desk Mac')).not.toBeNull()
  expect(await screen.findByText('Demo session')).not.toBeNull()
  expect(screen.getByText('/repo')).not.toBeNull()
  expect(screen.getByText('running')).not.toBeNull()
  expect(screen.getByText('Relay RPC ok')).not.toBeNull()
  expect(screen.getByText('WebRTC RPC ok')).not.toBeNull()
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server/web
npm test -- deviceListPage.test.tsx
```

Expected: fails because props and UI do not exist.

- [ ] **Step 3: Update `DeviceListPage` props**

Modify props:

```ts
import type { ConnectionsApi } from '../remote/remoteDeviceSessionController.js'
import {
  createRemoteDeviceSessionController,
  initialRemoteDeviceSessionSnapshot,
  type RemoteDeviceSessionSnapshot
} from '../remote/remoteDeviceSessionController.js'

type DeviceListPageProps = {
  devicesApi: DeviceListApi
  connectionsApi: ConnectionsApi
  createRemoteSession?: typeof createRemoteDeviceSessionController
  t: (key: string) => string
  onSelectDevice(device: RemoteDevice): void
  onUnauthorized?(): void
}
```

- [ ] **Step 4: Start one remote session controller for first online device**

Add state:

```ts
const [remoteSnapshot, setRemoteSnapshot] = useState<RemoteDeviceSessionSnapshot>(initialRemoteDeviceSessionSnapshot)
const remoteControllerRef = useRef<{ start(): Promise<void>; close(): void } | null>(null)
```

After `setDevices(response.list)`, start controller:

```ts
const firstOnline = response.list.find((device) => device.online)
remoteControllerRef.current?.close()
remoteControllerRef.current = null
setRemoteSnapshot(initialRemoteDeviceSessionSnapshot)
if (firstOnline) {
  const controller = (createRemoteSession ?? createRemoteDeviceSessionController)({
    deviceId: firstOnline.id,
    clientId: getStableClientId(),
    connectionsApi,
    onSnapshot: setRemoteSnapshot
  })
  remoteControllerRef.current = controller
  void controller.start()
}
```

Reuse the same stable client id logic currently in `DeviceConsolePage`; if it remains local there, move it to `remote-server/web/src/remote/remoteClientId.ts`.

- [ ] **Step 5: Render diagnostics and sessions**

Add a section under the device table:

```tsx
<section className="remote-sessions device-list-remote-sessions" aria-label={t('remote_sessions')}>
  <div className="panel-title">
    {t('remote_sessions')}
    <span className={`relay-status relay-status-${remoteSnapshot.relay.rpc}`}>
      {t('relay_rpc_status')}: {t(`remote_rpc_status_${remoteSnapshot.relay.rpc}`)}
    </span>
    <span className={`relay-status relay-status-${remoteSnapshot.webrtc.rpc}`}>
      {t('webrtc_rpc_status')}: {t(`remote_rpc_status_${remoteSnapshot.webrtc.rpc}`)}
    </span>
  </div>
  {remoteSnapshot.sessions.status === 'loading' ? <p className="state-message">{t('waiting_for_response')}</p> : null}
  {remoteSnapshot.sessions.status === 'error' ? (
    <p className="state-message state-message-error" role="alert">
      {remoteSnapshot.sessions.error ?? t('remote_sessions_failed')}
    </p>
  ) : null}
  {remoteSnapshot.sessions.status === 'ready' && remoteSnapshot.sessions.value ? (
    <RemoteSessionGroupsView page={remoteSnapshot.sessions.value} emptyText={t('remote_sessions_empty')} />
  ) : null}
</section>
```

- [ ] **Step 6: Add i18n keys in all supported languages**

Add keys to every language in `remote-server/web/src/i18n/messages.ts`:

```ts
relay_rpc_status: 'Relay RPC',
webrtc_rpc_status: 'WebRTC RPC',
remote_rpc_status_idle: 'idle',
remote_rpc_status_checking: 'checking',
remote_rpc_status_ok: 'ok',
remote_rpc_status_timeout: 'timeout',
remote_rpc_status_error: 'error',
remote_channel_unavailable: 'Remote channel unavailable'
```

Translate these for zh-CN, zh-TW, en, ja, ko, de. Keep short words so badges do not overflow.

- [ ] **Step 7: Update `App.tsx`**

Pass `connectionsApi`:

```tsx
<DeviceListPage
  devicesApi={devicesApi}
  connectionsApi={connectionsApi}
  t={t}
  onSelectDevice={setSelectedDevice}
  onUnauthorized={clearSession}
/>
```

- [ ] **Step 8: Verify tests**

Run:

```bash
cd remote-server/web
npm test -- deviceListPage.test.tsx
```

Expected: tests pass.

- [ ] **Step 9: Commit**

```bash
git add remote-server/web/src/devices/deviceListPage.tsx \
  remote-server/web/src/App.tsx \
  remote-server/web/src/i18n/messages.ts \
  remote-server/web/src/__tests__/deviceListPage.test.tsx
git commit -m "feat: 设备列表显示远程会话和通道诊断" \
  -m "修改内容：设备列表页自动连接第一台在线设备，展示 relay/WebRTC RPC 诊断和远程 session 列表。" \
  -m "修改原因：打开设备列表页即可验证远程会话闭环，不再必须进入控制台后人工判断。"
```

---

### Task 5: Reuse Shared Types in the Device Console and Preserve Debug Panels

**Files:**
- Modify: `remote-server/web/src/remote/deviceConsolePage.tsx`
- Test: `remote-server/web/src/__tests__/deviceConsolePage.test.tsx`

- [ ] **Step 1: Add test that console still displays sessions after extraction**

Ensure this existing test still passes and extend it if needed:

```ts
it('opens relay after accept, sends console RPC requests, and renders JSON responses', async () => {
  // Keep existing test body.
  expect(screen.getByText('Demo session')).not.toBeNull()
  expect(screen.getByText('running')).not.toBeNull()
})
```

- [ ] **Step 2: Replace console local rendering with `RemoteSessionGroupsView`**

If Task 1 did not fully replace all local rendering, complete that replacement here. Keep raw debug panels unchanged.

- [ ] **Step 3: Verify console tests**

Run:

```bash
cd remote-server/web
npm test -- deviceConsolePage.test.tsx
```

Expected: all console tests pass.

- [ ] **Step 4: Commit if there are changes**

```bash
git add remote-server/web/src/remote/deviceConsolePage.tsx remote-server/web/src/__tests__/deviceConsolePage.test.tsx
git commit -m "refactor: 控制台复用远程会话展示组件" \
  -m "修改内容：设备控制台复用共享 session 类型和列表展示组件，保留现有调试面板。" \
  -m "修改原因：确保设备列表页和控制台页展示一致，降低后续维护成本。"
```

---

### Task 6: Full Verification, Docker Deployment, and Browser Acceptance

**Files:**
- No source edits expected unless verification finds a bug.

- [ ] **Step 1: Run full web verification**

Run:

```bash
cd remote-server/web
npm test && npm run build
```

Expected:

- All Vitest files pass.
- Vite build succeeds.
- `tsc -p tsconfig.json --noEmit` succeeds.

- [ ] **Step 2: Run remote-server verification**

Run:

```bash
cd remote-server
npm run build && npm test
```

Expected:

- TypeScript build succeeds.
- All remote-server tests pass.

- [ ] **Step 3: Rebuild Docker remote-server**

Run:

```bash
cd remote-server
docker compose up -d --build remote-server
```

Expected: `remote-server-remote-server-1` is rebuilt and started.

- [ ] **Step 4: Check service health and device presence**

Run:

```bash
curl -sS http://127.0.0.1:27880/api/v1/health
docker logs --tail 80 remote-server-remote-server-1
docker exec remote-server-redis-1 sh -lc 'for k in $(redis-cli --scan --pattern "presence:device:*"); do echo $k; redis-cli ttl $k; redis-cli get $k; done'
```

Expected:

- Health response includes `"status":"ok"`.
- Logs do not show startup errors.
- Redis has a `presence:device:*` key with positive TTL while the local app is running.

- [ ] **Step 5: Browser acceptance with in-app browser**

Use `browser:control-in-app-browser` and open:

```text
http://127.0.0.1:27880/
```

Verify visible page state:

- User is logged in, or log in with the existing test account if token expired.
- Device list is visible.
- `NiuMa Device` or the current bound device name is visible.
- Device state shows online.
- Remote sessions section appears on the device list page without clicking the device.
- Relay RPC status shows ok.
- WebRTC RPC status shows ok.
- Session list shows real sessions, or the empty state text if the local app has no sessions.

- [ ] **Step 6: Browser console page acceptance**

In the same browser:

- Click the device connect button.
- Confirm the device console opens.
- Confirm relay and WebRTC status are visible.
- Confirm session list is visible and matches device-list data.
- Confirm Ping and remote state no longer remain permanently at `Plain RPC request timed out`.

- [ ] **Step 7: Use computer-use only if needed**

If the local Tauri app is not bound, remote access is disabled, or the local window must be operated, use `computer-use:computer-use` to:

- Open NiuMaNotifier settings.
- Confirm remote access is enabled.
- Confirm the device is bound.
- Trigger login/binding only if needed.

- [ ] **Step 8: Final status**

Report:

- Commits created.
- Test commands and results.
- Docker rebuild result.
- Browser acceptance result.
- Any residual risks, especially if WebRTC cannot become ok due to local network/browser environment.

