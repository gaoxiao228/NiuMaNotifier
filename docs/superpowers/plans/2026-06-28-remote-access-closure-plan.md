# Remote Access Closure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first externally usable remote-access loop: Web login, device list, online state, connection invite/accept, relay ping/pong, and minimal RPC for `rpc.ping`, `state.get`, and `session.list`.

**Architecture:** Reuse the existing `remote-server` auth, devices, connections, `/ws/device`, `/ws/client`, `/ws/relay`, local RemoteAgent signaling, and remote RPC skeleton. Add the missing Web console UI and wire the server/local message return path so a browser can drive the flow end to end. Use relay as the first stable transport; WebRTC and full E2EE hardening remain follow-up work after the MVP loop is manually verifiable.

**Tech Stack:** TypeScript, Fastify, PostgreSQL, Redis, React, Vite, Vitest, Rust, Tauri, Tokio, existing `niuma-core` remote modules.

---

## Scope Check

This plan implements the closure MVP from `docs/superpowers/specs/2026-06-28-remote-access-closure-design.md` through `session.list`.

Included:

- Web console React shell under `remote-server/web`.
- Web login and device list.
- Device list online state display.
- Connection creation from Web.
- `/ws/client` bidirectional signaling response path.
- `/ws/device` forwarding of `connection.accept`, `connection.reject`, `signal.*`, and `relay.bind` messages.
- Relay ping/pong transport between Web and local RemoteAgent.
- Minimal plaintext-in-frame RPC over relay for MVP: `rpc.ping`, `state.get`, `session.list`.
- Manual verification checklist.

Deferred:

- WebRTC DataChannel.
- Full E2EE handshake and device identity signature enforcement.
- Full device console UI.
- `session.detail`, `session.send_instruction`, `session.interrupt`, and interaction methods.
- Full audit UI and admin UI.

## Current State Notes

Already present:

- `remote-server/src/modules/auth/*`
- `remote-server/src/modules/devices/*`
- `remote-server/src/modules/connections/*`
- `remote-server/src/ws/device-socket.ts`
- `remote-server/src/ws/client-socket.ts`
- `remote-server/src/ws/relay-socket.ts`
- `remote-server/web/src/remote/e2ee/*`
- `src-tauri/src/remote/device_socket.rs`
- `src-tauri/src/remote/signaling.rs`
- `src-tauri/src/remote/webrtc_transport.rs`
- `crates/niuma-core/src/remote/rpc_envelope.rs`
- `crates/niuma-core/src/remote/transport.rs`

Important gap:

- Server device socket currently treats non-hello device messages as heartbeat/presence only. It must route `connection.accept`, `connection.reject`, `signal.*`, and relay/RPC control messages back to the bound Web client.
- Web package currently has no React app, routing, auth store, devices API, or connection UI.

## File Structure

Create:

- `remote-server/web/index.html`
- `remote-server/web/src/main.tsx`
- `remote-server/web/src/App.tsx`
- `remote-server/web/src/api/httpClient.ts`
- `remote-server/web/src/api/authApi.ts`
- `remote-server/web/src/api/devicesApi.ts`
- `remote-server/web/src/api/connectionsApi.ts`
- `remote-server/web/src/auth/authStore.ts`
- `remote-server/web/src/i18n/index.ts`
- `remote-server/web/src/i18n/messages.ts`
- `remote-server/web/src/devices/deviceListPage.tsx`
- `remote-server/web/src/remote/connectionClient.ts`
- `remote-server/web/src/remote/relayTransport.ts`
- `remote-server/web/src/remote/plainRpcClient.ts`
- `remote-server/web/src/remote/deviceConsolePage.tsx`
- `remote-server/web/src/shared/envelope.ts`
- `remote-server/web/src/shared/statusText.ts`
- `remote-server/web/src/styles.css`
- `remote-server/web/src/__tests__/httpClient.test.ts`
- `remote-server/web/src/__tests__/authStore.test.ts`
- `remote-server/web/src/__tests__/connectionClient.test.ts`
- `remote-server/web/src/__tests__/plainRpcClient.test.ts`
- `src-tauri/src/remote/relay_transport.rs`
- `src-tauri/src/remote/rpc_router.rs`
- `src-tauri/src/remote/relay_runtime.rs`

Modify:

- `remote-server/web/package.json`
- `remote-server/web/tsconfig.json`
- `remote-server/web/vitest.config.ts`
- `remote-server/src/modules/devices/device-socket-registry.ts`
- `remote-server/src/ws/client-socket.ts`
- `remote-server/src/ws/device-socket.ts`
- `remote-server/src/ws/ws-message.schemas.ts`
- `src-tauri/src/remote/mod.rs`
- `src-tauri/Cargo.toml`
- `src-tauri/src/remote/device_socket.rs`
- `src-tauri/src/remote/signaling.rs`
- `src-tauri/src/remote/agent.rs`
- `crates/niuma-core/src/remote/rpc_envelope.rs`
- `crates/niuma-core/src/remote/transport.rs`

## Port Policy

If running Web console dev server separately, use port `27881`, not Vite default `5173`.

Set `remote-server/web/package.json` scripts:

```json
{
  "scripts": {
    "dev": "vite --host 127.0.0.1 --port 27881",
    "build": "vite build && tsc -p tsconfig.json --noEmit",
    "test": "vitest run"
  }
}
```

## Task 1: Web Console Shell, I18n, And API Envelope

**Files:**
- Modify: `remote-server/web/package.json`
- Modify: `remote-server/web/tsconfig.json`
- Modify: `remote-server/web/vitest.config.ts`
- Create: `remote-server/web/index.html`
- Create: `remote-server/web/src/main.tsx`
- Create: `remote-server/web/src/App.tsx`
- Create: `remote-server/web/src/i18n/index.ts`
- Create: `remote-server/web/src/i18n/messages.ts`
- Create: `remote-server/web/src/shared/envelope.ts`
- Create: `remote-server/web/src/styles.css`
- Test: `remote-server/web/src/__tests__/httpClient.test.ts`

- [ ] **Step 1: Install Web UI dependencies**

Run:

```bash
cd remote-server/web
npm install react@19 react-dom@19 lucide-react@0.468.0
npm install -D @vitejs/plugin-react@latest jsdom@latest @testing-library/react@latest @testing-library/jest-dom@latest
```

Expected: `package.json` and `package-lock.json` update.

- [ ] **Step 2: Update Web package scripts**

Set `remote-server/web/package.json` scripts to:

```json
{
  "dev": "vite --host 127.0.0.1 --port 27881",
  "build": "vite build && tsc -p tsconfig.json --noEmit",
  "test": "vitest run"
}
```

- [ ] **Step 3: Configure Vitest for React**

Update `remote-server/web/vitest.config.ts`:

```ts
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vitest/config'

export default defineConfig({
  plugins: [react()],
  test: {
    environment: 'jsdom',
    globals: true
  }
})
```

- [ ] **Step 4: Create i18n messages**

Create `remote-server/web/src/i18n/messages.ts`:

```ts
export const supportedLanguages = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'de'] as const
export type SupportedLanguage = (typeof supportedLanguages)[number]

export const messages: Record<SupportedLanguage, Record<string, string>> = {
  'zh-CN': {
    app_title: 'NiuMaNotifier 远程控制台',
    login: '登录',
    email: '邮箱',
    password: '密码',
    devices: '设备',
    online: '在线',
    offline: '离线',
    connect: '连接',
    connecting: '连接中',
    connected: '已连接',
    relay: 'relay',
    state: '状态',
    sessions: '会话'
  },
  'zh-TW': {
    app_title: 'NiuMaNotifier 遠端控制台',
    login: '登入',
    email: '電子郵件',
    password: '密碼',
    devices: '裝置',
    online: '在線',
    offline: '離線',
    connect: '連線',
    connecting: '連線中',
    connected: '已連線',
    relay: 'relay',
    state: '狀態',
    sessions: '會話'
  },
  en: {
    app_title: 'NiuMaNotifier Remote Console',
    login: 'Sign in',
    email: 'Email',
    password: 'Password',
    devices: 'Devices',
    online: 'Online',
    offline: 'Offline',
    connect: 'Connect',
    connecting: 'Connecting',
    connected: 'Connected',
    relay: 'relay',
    state: 'State',
    sessions: 'Sessions'
  },
  ja: {
    app_title: 'NiuMaNotifier リモートコンソール',
    login: 'ログイン',
    email: 'メール',
    password: 'パスワード',
    devices: 'デバイス',
    online: 'オンライン',
    offline: 'オフライン',
    connect: '接続',
    connecting: '接続中',
    connected: '接続済み',
    relay: 'relay',
    state: '状態',
    sessions: 'セッション'
  },
  ko: {
    app_title: 'NiuMaNotifier 원격 콘솔',
    login: '로그인',
    email: '이메일',
    password: '비밀번호',
    devices: '기기',
    online: '온라인',
    offline: '오프라인',
    connect: '연결',
    connecting: '연결 중',
    connected: '연결됨',
    relay: 'relay',
    state: '상태',
    sessions: '세션'
  },
  de: {
    app_title: 'NiuMaNotifier Remote-Konsole',
    login: 'Anmelden',
    email: 'E-Mail',
    password: 'Passwort',
    devices: 'Geräte',
    online: 'Online',
    offline: 'Offline',
    connect: 'Verbinden',
    connecting: 'Verbindung läuft',
    connected: 'Verbunden',
    relay: 'relay',
    state: 'Status',
    sessions: 'Sitzungen'
  }
}
```

Create `remote-server/web/src/i18n/index.ts`:

```ts
import { messages, type SupportedLanguage, supportedLanguages } from './messages.js'

export function detectLanguage(language = navigator.language): SupportedLanguage {
  if (supportedLanguages.includes(language as SupportedLanguage)) return language as SupportedLanguage
  const base = language.split('-')[0]
  if (base === 'zh') return 'zh-CN'
  if (supportedLanguages.includes(base as SupportedLanguage)) return base as SupportedLanguage
  return 'en'
}

export function createTranslator(language = detectLanguage()) {
  const table = messages[language]
  return (key: string) => table[key] ?? messages.en[key] ?? key
}
```

- [ ] **Step 5: Create API envelope helper**

Create `remote-server/web/src/shared/envelope.ts`:

```ts
export type ApiEnvelope<T> = {
  code: number
  message: string
  data: T | null
}

export class ApiError extends Error {
  constructor(public code: number, message: string) {
    super(message)
  }
}

export function unwrapEnvelope<T>(payload: ApiEnvelope<T>): T {
  if (payload.code !== 0) throw new ApiError(payload.code, payload.message)
  if (payload.data == null) throw new ApiError(900001, '服务端成功响应缺少 data')
  return payload.data
}
```

- [ ] **Step 6: Write failing envelope test**

Create `remote-server/web/src/__tests__/httpClient.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { ApiError, unwrapEnvelope } from '../shared/envelope.js'

describe('api envelope', () => {
  it('unwraps success data and throws business errors', () => {
    expect(unwrapEnvelope({ code: 0, message: 'ok', data: { value: 1 } })).toEqual({ value: 1 })
    expect(() => unwrapEnvelope({ code: 200001, message: '未登录', data: null })).toThrow(ApiError)
  })
})
```

- [ ] **Step 7: Create React shell**

Create `remote-server/web/index.html`:

```html
<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>NiuMaNotifier Remote Console</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Create `remote-server/web/src/App.tsx`:

```tsx
import { createTranslator } from './i18n/index.js'

const t = createTranslator()

export function App() {
  return (
    <main className="app-shell">
      <header className="topbar">
        <h1>{t('app_title')}</h1>
      </header>
    </main>
  )
}
```

Create `remote-server/web/src/main.tsx`:

```tsx
import React from 'react'
import { createRoot } from 'react-dom/client'
import { App } from './App.js'
import './styles.css'

createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>
)
```

Create `remote-server/web/src/styles.css` with a dense console layout:

```css
:root {
  color-scheme: light;
  font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
  color: #172033;
  background: #f4f6fa;
}

body {
  margin: 0;
}

.app-shell {
  min-height: 100vh;
}

.topbar {
  height: 56px;
  display: flex;
  align-items: center;
  padding: 0 20px;
  border-bottom: 1px solid #dce3ef;
  background: #ffffff;
}

.topbar h1 {
  margin: 0;
  font-size: 18px;
  font-weight: 700;
  letter-spacing: 0;
}
```

- [ ] **Step 8: Run Web tests and build**

Run:

```bash
cd remote-server/web
npm test
npm run build
```

Expected: tests pass and Vite build succeeds.

- [ ] **Step 9: Commit**

```bash
git add remote-server/web/package.json remote-server/web/package-lock.json remote-server/web/index.html remote-server/web/src remote-server/web/tsconfig.json remote-server/web/vitest.config.ts
git commit -m "feat: 新增远程 Web 控制台骨架" -m "修改内容：新增 React 控制台入口、国际化文案和统一 API envelope 解析。" -m "修改原因：远程访问闭环需要外部 Web 客户端作为设备列表和控制台入口。"
```

## Task 2: Auth Store, HTTP Client, And Device List Page

**Files:**
- Create: `remote-server/web/src/api/httpClient.ts`
- Create: `remote-server/web/src/api/authApi.ts`
- Create: `remote-server/web/src/api/devicesApi.ts`
- Create: `remote-server/web/src/auth/authStore.ts`
- Create: `remote-server/web/src/devices/deviceListPage.tsx`
- Modify: `remote-server/web/src/App.tsx`
- Test: `remote-server/web/src/__tests__/authStore.test.ts`

- [ ] **Step 1: Write failing auth store test**

Create `remote-server/web/src/__tests__/authStore.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { createMemoryAuthStore } from '../auth/authStore.js'

describe('auth store', () => {
  it('stores access token and clears it on logout', () => {
    const store = createMemoryAuthStore()
    store.setToken('atk_1')
    expect(store.getToken()).toBe('atk_1')
    store.clear()
    expect(store.getToken()).toBeNull()
  })
})
```

- [ ] **Step 2: Implement auth store**

Create `remote-server/web/src/auth/authStore.ts`:

```ts
export type AuthStore = {
  getToken(): string | null
  setToken(token: string): void
  clear(): void
}

export function createLocalStorageAuthStore(key = 'niuma_remote_access_token'): AuthStore {
  return {
    getToken() {
      return localStorage.getItem(key)
    },
    setToken(token) {
      localStorage.setItem(key, token)
    },
    clear() {
      localStorage.removeItem(key)
    }
  }
}

export function createMemoryAuthStore(): AuthStore {
  let token: string | null = null
  return {
    getToken: () => token,
    setToken(value) {
      token = value
    },
    clear() {
      token = null
    }
  }
}
```

- [ ] **Step 3: Implement HTTP client**

Create `remote-server/web/src/api/httpClient.ts`:

```ts
import { unwrapEnvelope, type ApiEnvelope } from '../shared/envelope.js'
import type { AuthStore } from '../auth/authStore.js'

export function createHttpClient(options: { authStore: AuthStore; baseUrl?: string }) {
  const baseUrl = options.baseUrl ?? ''
  return {
    async get<T>(path: string): Promise<T> {
      const token = options.authStore.getToken()
      const response = await fetch(`${baseUrl}${path}`, {
        headers: token ? { authorization: `Bearer ${token}` } : {}
      })
      return unwrapEnvelope((await response.json()) as ApiEnvelope<T>)
    },
    async post<T>(path: string, body: unknown): Promise<T> {
      const token = options.authStore.getToken()
      const response = await fetch(`${baseUrl}${path}`, {
        method: 'POST',
        headers: {
          'content-type': 'application/json',
          ...(token ? { authorization: `Bearer ${token}` } : {})
        },
        body: JSON.stringify(body)
      })
      return unwrapEnvelope((await response.json()) as ApiEnvelope<T>)
    }
  }
}
```

- [ ] **Step 4: Implement auth and devices API wrappers**

Create `remote-server/web/src/api/authApi.ts`:

```ts
import type { createHttpClient } from './httpClient.js'

export type LoginResult = {
  access_token: string
  refresh_token: string
  expires_in: number
  user: { id: string; email: string; role: 'admin' | 'user' }
}

export function createAuthApi(http: ReturnType<typeof createHttpClient>) {
  return {
    login(email: string, password: string) {
      return http.post<LoginResult>('/api/v1/auth/login', { email, password })
    }
  }
}
```

Create `remote-server/web/src/api/devicesApi.ts`:

```ts
import type { createHttpClient } from './httpClient.js'

export type DeviceListItem = {
  id: string
  name: string
  online: boolean
  last_seen_at: string | null
  capabilities: {
    supports_webrtc?: boolean
    supports_relay?: boolean
    supports_remote_control?: boolean
  } | null
}

export type DeviceListResult = {
  list: DeviceListItem[]
}

export function createDevicesApi(http: ReturnType<typeof createHttpClient>) {
  return {
    list() {
      return http.get<DeviceListResult>('/api/v1/devices/list')
    }
  }
}
```

- [ ] **Step 5: Create device list page**

Create `remote-server/web/src/devices/deviceListPage.tsx`:

```tsx
import { RefreshCw } from 'lucide-react'
import { useEffect, useState } from 'react'
import type { DeviceListItem } from '../api/devicesApi.js'
import { createTranslator } from '../i18n/index.js'

const t = createTranslator()

export function DeviceListPage(props: {
  loadDevices(): Promise<{ list: DeviceListItem[] }>
  onConnect(device: DeviceListItem): void
}) {
  const [devices, setDevices] = useState<DeviceListItem[]>([])
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')

  async function refresh() {
    setLoading(true)
    setError('')
    try {
      const result = await props.loadDevices()
      setDevices(result.list)
    } catch (error) {
      setError(error instanceof Error ? error.message : String(error))
    } finally {
      setLoading(false)
    }
  }

  useEffect(() => {
    void refresh()
  }, [])

  return (
    <section className="page">
      <div className="page-header">
        <h2>{t('devices')}</h2>
        <button className="icon-button" onClick={refresh} disabled={loading} title="refresh">
          <RefreshCw size={18} />
        </button>
      </div>
      {error ? <p className="error-text">{error}</p> : null}
      <div className="device-table">
        {devices.map((device) => (
          <div className="device-row" key={device.id}>
            <div>
              <strong>{device.name}</strong>
              <span className={device.online ? 'status-online' : 'status-offline'}>
                {device.online ? t('online') : t('offline')}
              </span>
            </div>
            <button disabled={!device.online} onClick={() => props.onConnect(device)}>
              {t('connect')}
            </button>
          </div>
        ))}
      </div>
    </section>
  )
}
```

- [ ] **Step 6: Wire App login and devices flow**

Update `remote-server/web/src/App.tsx`:

```tsx
import { useMemo, useState } from 'react'
import { createAuthApi } from './api/authApi.js'
import { createDevicesApi, type DeviceListItem } from './api/devicesApi.js'
import { createHttpClient } from './api/httpClient.js'
import { createLocalStorageAuthStore } from './auth/authStore.js'
import { DeviceListPage } from './devices/deviceListPage.js'
import { createTranslator } from './i18n/index.js'

const t = createTranslator()
const authStore = createLocalStorageAuthStore()

export function App() {
  const [token, setToken] = useState(authStore.getToken())
  const [selectedDevice, setSelectedDevice] = useState<DeviceListItem | null>(null)
  const http = useMemo(() => createHttpClient({ authStore }), [token])
  const auth = useMemo(() => createAuthApi(http), [http])
  const devices = useMemo(() => createDevicesApi(http), [http])

  async function login(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const form = new FormData(event.currentTarget)
    const result = await auth.login(String(form.get('email')), String(form.get('password')))
    authStore.setToken(result.access_token)
    setToken(result.access_token)
  }

  if (!token) {
    return (
      <main className="login-page">
        <form className="login-panel" onSubmit={login}>
          <h1>{t('app_title')}</h1>
          <label>{t('email')}<input name="email" type="email" required /></label>
          <label>{t('password')}<input name="password" type="password" required minLength={8} /></label>
          <button>{t('login')}</button>
        </form>
      </main>
    )
  }

  return (
    <main className="app-shell">
      <header className="topbar">
        <h1>{t('app_title')}</h1>
      </header>
      {selectedDevice ? (
        <pre>{JSON.stringify(selectedDevice, null, 2)}</pre>
      ) : (
        <DeviceListPage loadDevices={() => devices.list()} onConnect={setSelectedDevice} />
      )}
    </main>
  )
}
```

- [ ] **Step 7: Run checks**

Run:

```bash
cd remote-server/web
npm test
npm run build
```

Expected: tests pass and build succeeds.

- [ ] **Step 8: Manual verification**

Run:

```bash
cd remote-server/web
npm run dev
```

Open `http://127.0.0.1:27881`, login with an existing remote-server account, and verify the bound device appears.

- [ ] **Step 9: Commit**

```bash
git add remote-server/web
git commit -m "feat: 新增远程设备列表页面" -m "修改内容：新增 Web 登录、token 保存、设备列表 API 和设备在线状态页面。" -m "修改原因：外部客户端需要先发现并选择已绑定设备，才能进入远程控制闭环。"
```

## Task 3: Bidirectional Client Signaling On Server

**Files:**
- Modify: `remote-server/src/modules/devices/device-socket-registry.ts`
- Modify: `remote-server/src/modules/connections/connections.schemas.ts`
- Modify: `remote-server/src/ws/client-socket.ts`
- Modify: `remote-server/src/ws/device-socket.ts`
- Modify: `remote-server/src/ws/ws-message.schemas.ts`
- Test: `remote-server/tests/client-socket.test.ts`
- Test: `remote-server/tests/device-socket.test.ts`

- [ ] **Step 1: Write failing registry test**

Update `remote-server/tests/client-socket.test.ts` with:

```ts
it('routes device responses back to a bound client socket', () => {
  const registry = createDeviceSocketRegistry()
  const clientSend = vi.fn()
  registry.bindClient('conn_1', { close: vi.fn(), send: clientSend })

  expect(registry.sendToClient('conn_1', { type: 'connection.accept' })).toBe(true)
  expect(clientSend).toHaveBeenCalledWith(JSON.stringify({ type: 'connection.accept' }))
})
```

Expected initial failure: `bindClient` and `sendToClient` do not exist.

- [ ] **Step 2: Extend device socket registry**

Update `remote-server/src/modules/devices/device-socket-registry.ts`:

```ts
export type DeviceSocket = {
  close(code: number, reason: string): void
  send?(data: string): void
}

export function createDeviceSocketRegistry() {
  const sockets = new Map<string, DeviceSocket>()
  const clientSockets = new Map<string, DeviceSocket>()

  return {
    add(deviceId: string, socket: DeviceSocket) {
      sockets.set(deviceId, socket)
    },
    remove(deviceId: string) {
      sockets.delete(deviceId)
    },
    has(deviceId: string) {
      return sockets.has(deviceId)
    },
    closeDevice(deviceId: string, code: number, reason: string) {
      const socket = sockets.get(deviceId)
      if (!socket) return false
      socket.close(code, reason)
      sockets.delete(deviceId)
      return true
    },
    sendToDevice(deviceId: string, message: object) {
      const socket = sockets.get(deviceId)
      if (!socket?.send) return false
      socket.send(JSON.stringify(message))
      return true
    },
    bindClient(connectionId: string, socket: DeviceSocket) {
      clientSockets.set(connectionId, socket)
    },
    unbindClient(connectionId: string) {
      clientSockets.delete(connectionId)
    },
    sendToClient(connectionId: string, message: object) {
      const socket = clientSockets.get(connectionId)
      if (!socket?.send) return false
      socket.send(JSON.stringify(message))
      return true
    }
  }
}
```

- [ ] **Step 3: Bind `/ws/client` sockets**

Browser WebSocket cannot set custom `Authorization` headers. Keep HTTP APIs protected by Bearer access token, but bind `/ws/client` using the short-lived `connection_id + connection_token` pair returned by `POST /api/v1/connections/create`.

Update `remote-server/src/ws/client-socket.ts` so `/ws/client` does not call `requireAuth`. Instead, change `bindClientConnection` input to:

```ts
export async function bindClientConnection(input: {
  query: unknown
  tokenPepper: string
  state: { get(connectionId: string): Promise<ConnectionState | null> }
})
```

Inside `bindClientConnection`, keep these checks:

```ts
const parsed = connectionClientBindSchema.safeParse(input.query)
if (!parsed.success) return { ok: false, code: 100101, message: '连接参数无效' }

const state = await input.state.get(parsed.data.connection_id)
if (!state) return { ok: false, code: 220401, message: '连接不存在' }
if (new Date(state.expires_at).getTime() <= Date.now()) {
  return { ok: false, code: 220402, message: '连接已过期' }
}

const tokenService = createConnectionTokenService({ tokenPepper: input.tokenPepper })
if (!tokenService.verify(parsed.data.connection_token, state.token_hash)) {
  return { ok: false, code: 220403, message: '连接权限不足' }
}
```

Return the connection binding from Redis state:

```ts
return {
  ok: true,
  connection: {
    connectionId: state.connection_id,
    userId: state.user_id,
    deviceId: state.device_id,
    clientId: state.client_id
  }
}
```

After a successful bind, register the client socket:

```ts
registry.bindClient(bound.connection.connectionId, socket)

socket.on('close', () => {
  registry.unbindClient(bound.connection.connectionId)
})
```

- [ ] **Step 4: Forward device signaling responses**

Update `remote-server/src/ws/ws-message.schemas.ts` to accept device response messages:

```ts
export const deviceSignalResponseSchema = z.discriminatedUnion('type', [
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('connection.accept'),
    data: z.object({
      connection_id: z.string().min(1),
      transport: z.string().min(1)
    })
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('connection.reject'),
    data: z.object({
      connection_id: z.string().min(1),
      reason: z.string().min(1)
    })
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.answer'),
    data: z.object({
      connection_id: z.string().min(1),
      sdp: z.string()
    })
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.ice_candidate'),
    data: z.object({
      connection_id: z.string().min(1),
      candidate: z.string(),
      sdp_mid: z.string().nullable().optional(),
      sdp_mline_index: z.number().nullable().optional()
    })
  }),
  z.object({
    version: z.literal(1),
    id: z.string().min(1),
    type: z.literal('signal.cancel'),
    data: z.object({
      connection_id: z.string().min(1),
      reason: z.string().min(1)
    })
  })
])
```

Update `remote-server/src/ws/device-socket.ts` inside `handleDeviceMessage`:

```ts
const signal = deviceSignalResponseSchema.safeParse(message)
if (signal.success) {
  return {
    kind: 'forward_to_client' as const,
    connectionId: signal.data.data.connection_id,
    message: signal.data
  }
}
```

Change `handleDeviceMessage` return type to:

```ts
Promise<void | { kind: 'forward_to_client'; connectionId: string; message: object }>
```

In the socket message handler:

```ts
const result = await handleDeviceMessage(...)
if (result?.kind === 'forward_to_client') {
  registry.sendToClient(result.connectionId, result.message)
}
```

- [ ] **Step 5: Run tests**

Run:

```bash
cd remote-server
npm test -- client-socket.test.ts device-socket.test.ts
npm run build
```

Expected: tests and build pass.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/modules/devices/device-socket-registry.ts remote-server/src/modules/connections/connections.schemas.ts remote-server/src/ws/client-socket.ts remote-server/src/ws/device-socket.ts remote-server/src/ws/ws-message.schemas.ts remote-server/tests/client-socket.test.ts remote-server/tests/device-socket.test.ts
git commit -m "feat: 打通远程信令双向转发" -m "修改内容：为客户端 WebSocket 增加连接绑定，并将设备 accept、reject 和 signal 消息回传给 Web 客户端。" -m "修改原因：外部控制台创建连接后必须收到本机 RemoteAgent 的连接响应，才能形成可验收的连接闭环。"
```

## Task 4: Web Connection Client And Device Console Entry

**Files:**
- Create: `remote-server/web/src/api/connectionsApi.ts`
- Create: `remote-server/web/src/remote/connectionClient.ts`
- Create: `remote-server/web/src/remote/deviceConsolePage.tsx`
- Modify: `remote-server/web/src/App.tsx`
- Test: `remote-server/web/src/__tests__/connectionClient.test.ts`

- [ ] **Step 1: Write failing connection client test**

Create `remote-server/web/src/__tests__/connectionClient.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildClientSocketUrl } from '../remote/connectionClient.js'

describe('connection client', () => {
  it('builds client websocket url without putting access token in query', () => {
    const url = buildClientSocketUrl('http://127.0.0.1:27880', {
      connection_id: 'conn_1',
      connection_token: 'cnt_secret'
    })

    expect(url).toBe('ws://127.0.0.1:27880/ws/client?connection_id=conn_1&connection_token=cnt_secret')
    expect(url).not.toContain('access_token')
  })
})
```

- [ ] **Step 2: Implement connection API**

Create `remote-server/web/src/api/connectionsApi.ts`:

```ts
import type { createHttpClient } from './httpClient.js'

export type ConnectionCreateResult = {
  connection_id: string
  connection_token: string
  client_id: string
  device_id: string
  relay_url?: string
  expires_in: number
}

export function createConnectionsApi(http: ReturnType<typeof createHttpClient>) {
  return {
    create(deviceId: string, clientId: string) {
      return http.post<ConnectionCreateResult>('/api/v1/connections/create', {
        device_id: deviceId,
        client_id: clientId,
        transport_preference: 'relay_first'
      })
    }
  }
}
```

- [ ] **Step 3: Implement WebSocket connection client**

Create `remote-server/web/src/remote/connectionClient.ts`:

```ts
export type ConnectionBind = {
  connection_id: string
  connection_token: string
}

export function buildClientSocketUrl(baseUrl: string, bind: ConnectionBind) {
  const base = new URL(baseUrl)
  base.protocol = base.protocol === 'https:' ? 'wss:' : 'ws:'
  base.pathname = '/ws/client'
  base.search = new URLSearchParams({
    connection_id: bind.connection_id,
    connection_token: bind.connection_token
  }).toString()
  return base.toString()
}

export function createConnectionClient(options: {
  baseUrl: string
  bind: ConnectionBind
  onMessage(value: unknown): void
  onStatus(status: 'connecting' | 'accepted' | 'rejected' | 'closed'): void
}) {
  const socket = new WebSocket(buildClientSocketUrl(options.baseUrl, options.bind), [])
  options.onStatus('connecting')
  socket.addEventListener('open', () => {
    socket.send(JSON.stringify({
      version: 1,
      id: `msg_${crypto.randomUUID()}`,
      type: 'signal.cancel',
      data: { reason: 'relay_first_no_webrtc_offer' }
    }))
  })
  socket.addEventListener('message', (event) => {
    const value = JSON.parse(String(event.data))
    if (value.type === 'connection.accept') options.onStatus('accepted')
    if (value.type === 'connection.reject') options.onStatus('rejected')
    options.onMessage(value)
  })
  socket.addEventListener('close', () => options.onStatus('closed'))
  return {
    close() {
      socket.close()
    }
  }
}
```

The WebSocket URL includes only `connection_token`, which is short-lived and scoped to one connection. It must not include the account access token.

- [ ] **Step 4: Create device console shell**

Create `remote-server/web/src/remote/deviceConsolePage.tsx`:

```tsx
import { useState } from 'react'
import type { DeviceListItem } from '../api/devicesApi.js'
import type { ConnectionCreateResult } from '../api/connectionsApi.js'

export function DeviceConsolePage(props: {
  device: DeviceListItem
  createConnection(): Promise<ConnectionCreateResult>
}) {
  const [status, setStatus] = useState('idle')
  const [connectionId, setConnectionId] = useState('')

  async function connect() {
    setStatus('connecting')
    const result = await props.createConnection()
    setConnectionId(result.connection_id)
    setStatus('created')
  }

  return (
    <section className="console-grid">
      <header className="console-header">
        <strong>{props.device.name}</strong>
        <span>{status}</span>
        <button onClick={connect} disabled={!props.device.online}>连接</button>
      </header>
      <pre>{connectionId}</pre>
    </section>
  )
}
```

- [ ] **Step 5: Wire App selected device to console**

Update `remote-server/web/src/App.tsx` so selected device renders `DeviceConsolePage` and calls `connections.create(selectedDevice.id, clientId)`.

Use a stable client id:

```ts
function getClientId() {
  const key = 'niuma_remote_client_id'
  const existing = localStorage.getItem(key)
  if (existing) return existing
  const value = `web_${crypto.randomUUID()}`
  localStorage.setItem(key, value)
  return value
}
```

- [ ] **Step 6: Run Web checks**

Run:

```bash
cd remote-server/web
npm test
npm run build
```

Expected: tests and build pass.

- [ ] **Step 7: Commit**

```bash
git add remote-server/web
git commit -m "feat: 新增远程设备连接入口" -m "修改内容：新增连接创建 API、设备控制台入口和 WebSocket 连接客户端。" -m "修改原因：外部客户端需要从设备列表进入连接流程，才能触发本机 RemoteAgent 的连接邀请。"
```

## Task 5: Local Relay Transport And Relay Bind Handshake

**Files:**
- Create: `src-tauri/src/remote/relay_transport.rs`
- Create: `src-tauri/src/remote/relay_runtime.rs`
- Modify: `src-tauri/src/remote/mod.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/remote/signaling.rs`
- Modify: `src-tauri/src/remote/device_socket.rs`
- Test: `src-tauri/src/remote/relay_transport.rs`

- [ ] **Step 1: Write failing relay URL test**

Create `src-tauri/src/remote/relay_transport.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_relay_socket_url() {
        let url = relay_socket_url(
            "http://127.0.0.1:27880",
            "conn_1",
            "cnt_secret",
            RelaySide::Device,
        )
        .unwrap();

        assert_eq!(
            url,
            "ws://127.0.0.1:27880/ws/relay?connection_id=conn_1&connection_token=cnt_secret&side=device"
        );
    }
}
```

- [ ] **Step 2: Implement relay URL and frame types**

Add `base64` to `src-tauri/Cargo.toml`:

```toml
base64 = "0.22"
```

Replace `src-tauri/src/remote/relay_transport.rs` with:

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RelaySide {
    Client,
    Device,
}

impl RelaySide {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Client => "client",
            Self::Device => "device",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelayFrame {
    pub version: u8,
    pub connection_id: String,
    pub seq: u64,
    pub payload: String,
}

pub fn relay_socket_url(
    server_url: &str,
    connection_id: &str,
    connection_token: &str,
    side: RelaySide,
) -> Result<String, String> {
    let base = server_url.trim_end_matches('/');
    let ws_base = if base.starts_with("https://") {
        base.replacen("https://", "wss://", 1)
    } else if base.starts_with("http://") {
        base.replacen("http://", "ws://", 1)
    } else {
        return Err("远程服务地址必须使用 http 或 https".to_string());
    };
    Ok(format!(
        "{ws_base}/ws/relay?connection_id={connection_id}&connection_token={connection_token}&side={}",
        side.as_str()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_relay_socket_url() {
        let url = relay_socket_url(
            "http://127.0.0.1:27880",
            "conn_1",
            "cnt_secret",
            RelaySide::Device,
        )
        .unwrap();

        assert_eq!(
            url,
            "ws://127.0.0.1:27880/ws/relay?connection_id=conn_1&connection_token=cnt_secret&side=device"
        );
    }
}
```

- [ ] **Step 3: Export module**

Update `src-tauri/src/remote/mod.rs`:

```rust
pub mod relay_transport;
pub mod relay_runtime;
```

- [ ] **Step 4: Add relay bind instruction from signaling**

Update `src-tauri/src/remote/signaling.rs` so relay-compatible invites are accepted with `transport: "relay"` when `transport_preference` is `Relay` or `Auto`.

Update Web `connections.create` to request `relay_first`. Update server `createConnectionInviteMessage` so `relay_first` is sent to the device as core `transport_preference: "relay"` in `connection.invite`, matching `niuma_core::remote::signaling::TransportPreference::Relay`.

- [ ] **Step 5: Run local tests**

Run:

```bash
cargo test --manifest-path src-tauri/Cargo.toml remote::relay_transport remote::signaling
```

Expected: tests pass.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/src/remote/relay_transport.rs src-tauri/src/remote/relay_runtime.rs src-tauri/src/remote/mod.rs src-tauri/src/remote/signaling.rs src-tauri/src/remote/device_socket.rs
git commit -m "feat: 新增本机 relay 传输入口" -m "修改内容：新增 relay socket URL、relay 帧模型和本机 relay 连接准备逻辑。" -m "修改原因：远程访问闭环先使用 relay 作为稳定可验收传输，再接入 WebRTC 优先策略。"
```

## Task 6: Relay Ping/Pong Between Web And Local

**Files:**
- Create: `remote-server/web/src/remote/relayTransport.ts`
- Modify: `remote-server/src/ws/relay-socket.ts`
- Modify: `src-tauri/src/remote/relay_runtime.rs`
- Test: `remote-server/web/src/__tests__/connectionClient.test.ts`
- Test: `src-tauri/src/remote/relay_runtime.rs`

- [ ] **Step 1: Write Web relay frame test**

Update `remote-server/web/src/__tests__/connectionClient.test.ts`:

```ts
import { encodeRelayPayload, decodeRelayPayload } from '../remote/relayTransport.js'

it('encodes relay payload as base64 json', () => {
  const encoded = encodeRelayPayload({ type: 'ping' })
  expect(decodeRelayPayload(encoded)).toEqual({ type: 'ping' })
})
```

- [ ] **Step 2: Implement Web relay transport helpers**

Create `remote-server/web/src/remote/relayTransport.ts`:

```ts
export type RelayMessage = {
  version: 1
  connection_id: string
  seq: number
  payload: string
}

export function encodeRelayPayload(value: unknown) {
  return btoa(JSON.stringify(value))
}

export function decodeRelayPayload(payload: string) {
  return JSON.parse(atob(payload)) as unknown
}

export function buildRelaySocketUrl(baseUrl: string, input: {
  connection_id: string
  connection_token: string
  side: 'client' | 'device'
}) {
  const base = new URL(baseUrl)
  base.protocol = base.protocol === 'https:' ? 'wss:' : 'ws:'
  base.pathname = '/ws/relay'
  base.search = new URLSearchParams(input).toString()
  return base.toString()
}
```

- [ ] **Step 3: Allow browser relay client binding by connection token**

Browser WebSocket cannot send Bearer headers. Update `remote-server/src/ws/relay-socket.ts` so `side=client` authenticates only by `connection_id + connection_token`. Keep `side=device` authenticated by `Authorization: Device <device_token>`.

Change actor resolution for client side to:

```ts
const actor = parsedQuery.data.side === 'client'
  ? { ok: true as const, actor: { kind: 'client' as const, userId: '' } }
  : await resolveDeviceActor(request, deviceTokenService)
```

Then update `bindRelaySocket` so when `side === 'client'`, it verifies the connection token and uses `state.user_id` as the bound user instead of comparing an access-token user:

```ts
if (parsed.data.side === 'client' && input.actor.kind !== 'client') {
  return { ok: false, code: 220403, message: '连接权限不足' }
}
if (parsed.data.side === 'device' && (input.actor.kind !== 'device' || input.actor.deviceId !== state.device_id)) {
  return { ok: false, code: 220403, message: '连接权限不足' }
}
```

The token check remains mandatory:

```ts
if (!tokenService.verify(parsed.data.connection_token, state.token_hash)) {
  return { ok: false, code: 220403, message: '连接权限不足' }
}
```

- [ ] **Step 4: Implement local ping responder**

Create or update `src-tauri/src/remote/relay_runtime.rs`:

```rust
use crate::remote::relay_transport::RelayFrame;
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde_json::json;

pub fn handle_relay_payload(payload_base64: &str) -> Result<Option<String>, String> {
    let bytes = STANDARD
        .decode(payload_base64)
        .map_err(|error| format!("relay payload base64 解析失败：{error}"))?;
    let value: serde_json::Value = serde_json::from_slice(&bytes)
        .map_err(|error| format!("relay payload JSON 解析失败：{error}"))?;
    if value["type"] == "ping" {
        let response = serde_json::to_vec(&json!({ "type": "pong" }))
            .map_err(|error| format!("relay pong 序列化失败：{error}"))?;
        return Ok(Some(STANDARD.encode(response)));
    }
    Ok(None)
}

pub fn build_relay_response_frame(input: &RelayFrame, payload: String) -> RelayFrame {
    RelayFrame {
        version: 1,
        connection_id: input.connection_id.clone(),
        seq: input.seq + 1,
        payload,
    }
}
```

- [ ] **Step 5: Add local test**

Append to `src-tauri/src/remote/relay_runtime.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_to_ping_payload() {
        let ping = STANDARD.encode(br#"{"type":"ping"}"#);
        let pong = handle_relay_payload(&ping).unwrap().unwrap();
        let decoded = STANDARD.decode(pong).unwrap();
        assert_eq!(String::from_utf8(decoded).unwrap(), r#"{"type":"pong"}"#);
    }
}
```

- [ ] **Step 6: Wire relay runtime into relay socket loop**

Implement the async socket loop in `src-tauri/src/remote/relay_transport.rs` using `tokio_tungstenite::connect_async`:

- connect to `/ws/relay?...side=device`
- read text frames
- parse `RelayFrame`
- pass `payload` into `handle_relay_payload`
- send response frame when `Some(payload)` is returned

For `side=device`, reuse the existing device token authorization header: `Authorization: Device <device_token>`.

- [ ] **Step 7: Run checks**

Run:

```bash
cd remote-server/web
npm test
npm run build
cd ../..
cargo test --manifest-path src-tauri/Cargo.toml remote::relay_transport remote::relay_runtime
```

Expected: Web and Rust tests pass.

- [ ] **Step 8: Commit**

```bash
git add remote-server/src/ws/relay-socket.ts remote-server/web/src/remote/relayTransport.ts remote-server/web/src/__tests__/connectionClient.test.ts src-tauri/src/remote/relay_runtime.rs src-tauri/src/remote/relay_transport.rs
git commit -m "feat: 打通 relay ping pong 传输" -m "修改内容：新增 Web relay 帧编码、本机 relay ping 响应和 relay socket 读写循环。" -m "修改原因：远程访问闭环需要先验证 Web 与本机之间存在稳定可用的传输通道。"
```

## Task 7: Minimal Plain RPC Over Relay

**Files:**
- Create: `remote-server/web/src/remote/plainRpcClient.ts`
- Create: `src-tauri/src/remote/rpc_router.rs`
- Modify: `src-tauri/src/remote/relay_runtime.rs`
- Modify: `crates/niuma-core/src/remote/rpc_envelope.rs`
- Test: `remote-server/web/src/__tests__/plainRpcClient.test.ts`
- Test: `src-tauri/src/remote/rpc_router.rs`

- [ ] **Step 1: Write Web RPC test**

Create `remote-server/web/src/__tests__/plainRpcClient.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { createPlainRpcRequest, isPlainRpcResponse } from '../remote/plainRpcClient.js'

describe('plain rpc client', () => {
  it('creates request envelopes', () => {
    const request = createPlainRpcRequest('req_1', 'rpc.ping', {})
    expect(request).toEqual({
      version: 1,
      type: 'request',
      id: 'req_1',
      method: 'rpc.ping',
      params: {}
    })
  })

  it('recognizes response envelopes', () => {
    expect(isPlainRpcResponse({
      version: 1,
      type: 'response',
      id: 'req_1',
      ok: true,
      result: { pong: true }
    })).toBe(true)
  })
})
```

- [ ] **Step 2: Implement Web plain RPC helper**

Create `remote-server/web/src/remote/plainRpcClient.ts`:

```ts
export type PlainRpcRequest = {
  version: 1
  type: 'request'
  id: string
  method: string
  params: Record<string, unknown>
}

export type PlainRpcResponse = {
  version: 1
  type: 'response'
  id: string
  ok: boolean
  result?: unknown
  error?: { code: string; message: string }
}

export function createPlainRpcRequest(id: string, method: string, params: Record<string, unknown>): PlainRpcRequest {
  return { version: 1, type: 'request', id, method, params }
}

export function isPlainRpcResponse(value: unknown): value is PlainRpcResponse {
  return Boolean(
    value &&
    typeof value === 'object' &&
    (value as PlainRpcResponse).version === 1 &&
    (value as PlainRpcResponse).type === 'response' &&
    typeof (value as PlainRpcResponse).id === 'string'
  )
}
```

- [ ] **Step 3: Add Rust RPC router tests**

Create `src-tauri/src/remote/rpc_router.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replies_to_rpc_ping() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_1",
            "method": "rpc.ping",
            "params": {}
        }))
        .unwrap();

        assert_eq!(response["type"], "response");
        assert_eq!(response["id"], "req_1");
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["pong"], true);
    }
}
```

- [ ] **Step 4: Implement minimal Rust RPC router**

Replace `src-tauri/src/remote/rpc_router.rs` with:

```rust
use serde_json::{json, Value};

pub fn handle_plain_rpc(request: Value) -> Result<Value, String> {
    let id = request["id"]
        .as_str()
        .ok_or_else(|| "RPC 请求缺少 id".to_string())?;
    let method = request["method"]
        .as_str()
        .ok_or_else(|| "RPC 请求缺少 method".to_string())?;

    let result = match method {
        "rpc.ping" => json!({ "pong": true }),
        "state.get" => json!({ "state": "unknown", "source": "remote_mvp" }),
        "session.list" => json!({ "list": [] }),
        _ => {
            return Ok(json!({
                "version": 1,
                "type": "response",
                "id": id,
                "ok": false,
                "error": { "code": "method_not_found", "message": "远程 RPC 方法不存在" }
            }))
        }
    };

    Ok(json!({
        "version": 1,
        "type": "response",
        "id": id,
        "ok": true,
        "result": result
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn replies_to_rpc_ping() {
        let response = handle_plain_rpc(json!({
            "version": 1,
            "type": "request",
            "id": "req_1",
            "method": "rpc.ping",
            "params": {}
        }))
        .unwrap();

        assert_eq!(response["type"], "response");
        assert_eq!(response["id"], "req_1");
        assert_eq!(response["ok"], true);
        assert_eq!(response["result"]["pong"], true);
    }
}
```

- [ ] **Step 5: Wire RPC router into relay runtime**

Update `src-tauri/src/remote/relay_runtime.rs` so `handle_relay_payload`:

- decodes relay payload
- if payload `type` is `ping`, returns pong
- if payload `type` is `request`, calls `crate::remote::rpc_router::handle_plain_rpc`
- encodes the RPC response as relay payload

- [ ] **Step 6: Run checks**

Run:

```bash
cd remote-server/web
npm test
npm run build
cd ../..
cargo test --manifest-path src-tauri/Cargo.toml remote::rpc_router remote::relay_runtime
```

Expected: Web and Rust tests pass.

- [ ] **Step 7: Commit**

```bash
git add remote-server/web/src/remote/plainRpcClient.ts remote-server/web/src/__tests__/plainRpcClient.test.ts src-tauri/src/remote/rpc_router.rs src-tauri/src/remote/relay_runtime.rs src-tauri/src/remote/mod.rs crates/niuma-core/src/remote/rpc_envelope.rs
git commit -m "feat: 新增远程最小 RPC 闭环" -m "修改内容：新增 Web plain RPC 客户端、本机 RPC router，并通过 relay 返回 rpc.ping、state.get 和 session.list 响应。" -m "修改原因：外部客户端需要先读取真实本机状态和会话列表，才能进入完整远程控制台阶段。"
```

## Task 8: Device Console Shows Ping, State, And Sessions

**Files:**
- Modify: `remote-server/web/src/remote/deviceConsolePage.tsx`
- Modify: `remote-server/web/src/remote/relayTransport.ts`
- Modify: `remote-server/web/src/remote/plainRpcClient.ts`
- Test: `remote-server/web/src/__tests__/plainRpcClient.test.ts`

- [ ] **Step 1: Extend Web RPC client with pending requests**

Update `remote-server/web/src/remote/plainRpcClient.ts`:

```ts
export function createPlainRpcClient(options: {
  timeoutMs: number
  send(value: PlainRpcRequest): void
}) {
  const pending = new Map<string, { resolve(value: unknown): void; reject(error: Error): void; timer: ReturnType<typeof setTimeout> }>()

  return {
    request(method: string, params: Record<string, unknown>) {
      const id = `req_${crypto.randomUUID()}`
      const request = createPlainRpcRequest(id, method, params)
      const promise = new Promise<unknown>((resolve, reject) => {
        const timer = setTimeout(() => {
          pending.delete(id)
          reject(new Error('remote rpc timeout'))
        }, options.timeoutMs)
        pending.set(id, { resolve, reject, timer })
      })
      options.send(request)
      return promise
    },
    handle(value: unknown) {
      if (!isPlainRpcResponse(value)) return false
      const entry = pending.get(value.id)
      if (!entry) return false
      clearTimeout(entry.timer)
      pending.delete(value.id)
      if (value.ok) entry.resolve(value.result)
      else entry.reject(new Error(value.error?.message ?? 'remote rpc failed'))
      return true
    }
  }
}
```

- [ ] **Step 2: Update console page to call RPCs**

Update `remote-server/web/src/remote/deviceConsolePage.tsx` so it:

- creates connection
- opens relay socket as client
- sends `rpc.ping`
- sends `state.get`
- sends `session.list`
- renders returned JSON in two panels

Use the existing dense console CSS classes:

```tsx
<section className="console-grid">
  <header className="console-header">...</header>
  <section className="console-panel"><h2>状态</h2><pre>{JSON.stringify(state, null, 2)}</pre></section>
  <section className="console-panel"><h2>会话</h2><pre>{JSON.stringify(sessions, null, 2)}</pre></section>
</section>
```

- [ ] **Step 3: Run Web checks**

Run:

```bash
cd remote-server/web
npm test
npm run build
```

Expected: tests and build pass.

- [ ] **Step 4: Manual browser verification**

1. Start remote-server Docker at `http://127.0.0.1:27880`.
2. Start NiuMaNotifier Tauri dev build and confirm settings page is bound.
3. Start Web dev server:

```bash
cd remote-server/web
npm run dev
```

4. Open `http://127.0.0.1:27881`.
5. Login.
6. Confirm device is online.
7. Click connect.
8. Confirm relay status is connected.
9. Confirm ping returns pong.
10. Confirm state and sessions panels show JSON responses.

- [ ] **Step 5: Commit**

```bash
git add remote-server/web/src/remote remote-server/web/src/App.tsx remote-server/web/src/styles.css
git commit -m "feat: 展示远程最小控制台数据" -m "修改内容：设备控制台通过 relay RPC 展示 ping、state.get 和 session.list 结果。" -m "修改原因：远程访问 MVP 需要外部客户端能读取本机真实状态和会话列表，证明端到端控制通道可用。"
```

## Task 9: Verification Runbook

**Files:**
- Create: `docs/superpowers/plans/2026-06-28-remote-access-closure-verification.md`

- [ ] **Step 1: Create runbook**

Create `docs/superpowers/plans/2026-06-28-remote-access-closure-verification.md`:

```md
# Remote Access Closure Verification

## Services

Remote server:

```bash
cd remote-server
docker compose up --build
```

Web console dev server:

```bash
cd remote-server/web
npm run dev
```

Open:

```text
http://127.0.0.1:27881
```

## Expected Results

1. Login succeeds.
2. `/devices` shows the bound device.
3. Device shows online while NiuMaNotifier is running.
4. Clicking connect creates a connection.
5. Web console receives `connection.accept`.
6. Relay connects.
7. Ping returns pong.
8. `state.get` renders JSON.
9. `session.list` renders JSON.

## Failure Checks

- If device is offline, check NiuMaNotifier settings page RemoteAgent status.
- If connection create fails, check `/api/v1/devices/list` returns the selected device for the logged-in user.
- If relay does not connect, check `/ws/relay` query includes `connection_id`, `connection_token`, and `side=client`.
- If RPC times out, check the local relay socket is connected with `side=device`.
```

- [ ] **Step 2: Run full checks**

Run:

```bash
cd remote-server
npm run check
cd web
npm test
npm run build
cd ../../
cargo test -p niuma-core remote
cargo test --manifest-path src-tauri/Cargo.toml remote
```

Expected: all commands pass. Existing warnings are acceptable if they are already present and unrelated.

- [ ] **Step 3: Commit**

```bash
git add -f docs/superpowers/plans/2026-06-28-remote-access-closure-verification.md
git commit -m "docs: 新增远程访问闭环验收手册" -m "修改内容：新增远程 Web 控制台、设备在线、连接、relay 和最小 RPC 的手动验收步骤。" -m "修改原因：远程访问 MVP 需要稳定可重复的端到端验收路径，避免只依赖单元测试判断链路是否可用。"
```

## Final Verification

After all tasks are complete, run:

```bash
cd remote-server
npm run check
cd web
npm test
npm run build
cd ../../
cargo test -p niuma-core remote
cargo test --manifest-path src-tauri/Cargo.toml remote
```

Manual verification must cover:

- Browser login.
- Device online display.
- Connection accepted.
- Relay connected.
- `rpc.ping` returns pong.
- `state.get` returns a response.
- `session.list` returns a response.

## Self-Review

- Spec coverage: Tasks 1-2 cover Web login and device list; Task 3 covers bidirectional signaling; Tasks 4-6 cover connection and relay; Tasks 7-8 cover minimal RPC and console display; Task 9 covers manual verification.
- Placeholder scan: no unresolved implementation slots are intentionally left in this plan.
- Type consistency: `connection_id`, `connection_token`, `client_id`, `device_id`, `version`, `type`, `id`, `data`, `seq`, and `payload` match existing server and local protocol style.
- Scope control: WebRTC, full E2EE, complete console, audit UI, and advanced control methods are deferred to later plans after this MVP loop is manually verified.
