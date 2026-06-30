# Remote Server Relay Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the relay fallback slice for the remote server so Web client and RemoteAgent can exchange encrypted frames through `/ws/relay` when WebRTC DataChannel is unavailable.

**Architecture:** This plan builds on connection signaling. `/ws/relay` binds a socket to an existing `connection_id` using the short-lived `connection_token`, records a Redis `relay:{connection_id}` route, and forwards only opaque ciphertext frames between the client and device sides. The relay layer validates routing metadata and frame shape, but it never decrypts, parses, stores, or logs RPC payload plaintext.

**Tech Stack:** Node.js LTS, TypeScript, Fastify, `@fastify/websocket`, Redis/ioredis, Zod, Vitest.

---

## Scope Check

This plan covers:

- `/ws/relay` route registration.
- relay socket binding with `connection_id`, `connection_token`, and `side`.
- Redis `relay:{connection_id}` route state.
- relay frame schema validation.
- ciphertext-only forwarding between `client` and `device` sides.
- sequence number monotonicity per side.
- cleanup when either relay socket closes.

This plan does not cover:

- E2EE RPC session handshake.
- WebRTC success/failure decision logic in Web or RemoteAgent.
- RPC method routing or command execution.
- Connection status callbacks such as `connected`, `failed`, and `closed`.
- Multi-instance relay routing through Redis pub/sub. This plan targets the single-instance Docker Compose MVP.

## Protocol Notes

Relay WebSocket query parameters:

```text
/ws/relay?connection_id=conn_...&connection_token=cnt_...&side=client
/ws/relay?connection_id=conn_...&connection_token=cnt_...&side=device
```

Relay frame:

```json
{
  "version": 1,
  "type": "relay.frame",
  "id": "msg_001",
  "connection_id": "conn_...",
  "seq": 1,
  "ciphertext": "base64url-or-base64"
}
```

The server forwards the frame with the same `ciphertext`. It does not parse decrypted RPC content and does not write ciphertext to PostgreSQL.

## File Structure

Create:

- `remote-server/src/modules/relay/relay.schemas.ts` - relay bind and frame schemas.
- `remote-server/src/modules/relay/relay-route.service.ts` - Redis `relay:{connection_id}` route state.
- `remote-server/src/modules/relay/relay-socket-registry.ts` - in-process relay socket registry.
- `remote-server/src/ws/relay-socket.ts` - `/ws/relay` bind, frame validation, and forwarding.
- `remote-server/tests/relay-route.service.test.ts`
- `remote-server/tests/relay-socket-registry.test.ts`
- `remote-server/tests/relay-socket.test.ts`

Modify:

- `remote-server/src/app.ts` - register `/ws/relay` with existing dependency injection pattern.
- `remote-server/src/modules/connections/connection-state.service.ts` - expose a reusable connection state reader type if not already exported by previous plan.
- `remote-server/src/modules/connections/connection-token.service.ts` - reuse existing token verification from signaling plan.

## Task 1: Relay Schemas And Route State

**Files:**
- Create: `remote-server/src/modules/relay/relay.schemas.ts`
- Create: `remote-server/src/modules/relay/relay-route.service.ts`
- Test: `remote-server/tests/relay-route.service.test.ts`

- [ ] **Step 1: Write failing relay route tests**

Create `remote-server/tests/relay-route.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { relayBindSchema, relayFrameSchema } from '../src/modules/relay/relay.schemas.js'
import { createRelayRouteService, type RelayRouteRedis } from '../src/modules/relay/relay-route.service.js'

function createFakeRedis(): RelayRouteRedis {
  const values = new Map<string, string>()
  return {
    async set(key, value, mode, ttlSeconds) {
      expect(mode).toBe('EX')
      expect(ttlSeconds).toBe(120)
      values.set(key, value)
      return 'OK'
    },
    async get(key) {
      return values.get(key) ?? null
    },
    async del(key) {
      values.delete(key)
      return 1
    }
  }
}

describe('relay schemas and route state', () => {
  it('validates bind query and ciphertext frame', () => {
    expect(relayBindSchema.parse({
      connection_id: 'conn_1',
      connection_token: 'cnt_token_with_enough_length_123456',
      side: 'client'
    }).side).toBe('client')

    expect(relayFrameSchema.parse({
      version: 1,
      type: 'relay.frame',
      id: 'msg_1',
      connection_id: 'conn_1',
      seq: 1,
      ciphertext: 'YWJjZA=='
    }).ciphertext).toBe('YWJjZA==')
  })

  it('writes and deletes relay route state', async () => {
    const service = createRelayRouteService({ redis: createFakeRedis(), ttlSeconds: 120 })
    await service.setRoute({
      connectionId: 'conn_1',
      clientSocketId: 'sock_client',
      deviceSocketId: 'sock_device',
      serverInstanceId: 'srv_1',
      startedAt: '2026-06-28T00:00:00.000Z'
    })

    await expect(service.getRoute('conn_1')).resolves.toEqual({
      connection_id: 'conn_1',
      client_socket_id: 'sock_client',
      device_socket_id: 'sock_device',
      server_instance_id: 'srv_1',
      started_at: '2026-06-28T00:00:00.000Z'
    })

    await service.deleteRoute('conn_1')
    await expect(service.getRoute('conn_1')).resolves.toBeNull()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- relay-route.service.test.ts
```

Expected: FAIL because relay schema and route service files do not exist.

- [ ] **Step 3: Implement relay schemas**

Create `remote-server/src/modules/relay/relay.schemas.ts`:

```ts
import { z } from 'zod'

export const relayBindSchema = z.object({
  connection_id: z.string().min(1).max(160),
  connection_token: z.string().min(32),
  side: z.enum(['client', 'device'])
})

export const relayFrameSchema = z.object({
  version: z.literal(1),
  type: z.literal('relay.frame'),
  id: z.string().min(1).max(160),
  connection_id: z.string().min(1).max(160),
  seq: z.number().int().positive(),
  ciphertext: z.string().min(1).max(1024 * 1024)
})

export type RelayBindInput = z.infer<typeof relayBindSchema>
export type RelayFrame = z.infer<typeof relayFrameSchema>
export type RelaySide = RelayBindInput['side']
```

- [ ] **Step 4: Implement relay route service**

Create `remote-server/src/modules/relay/relay-route.service.ts`:

```ts
export type RelayRoute = {
  connection_id: string
  client_socket_id: string | null
  device_socket_id: string | null
  server_instance_id: string
  started_at: string
}

export type RelayRouteInput = {
  connectionId: string
  clientSocketId: string | null
  deviceSocketId: string | null
  serverInstanceId: string
  startedAt: string
}

export type RelayRouteRedis = {
  set(key: string, value: string, mode: 'EX', ttlSeconds: number): Promise<unknown>
  get(key: string): Promise<string | null>
  del(key: string): Promise<unknown>
}

function relayKey(connectionId: string) {
  return `relay:${connectionId}`
}

export function createRelayRouteService(options: { redis: RelayRouteRedis; ttlSeconds: number }) {
  return {
    async setRoute(input: RelayRouteInput) {
      const route: RelayRoute = {
        connection_id: input.connectionId,
        client_socket_id: input.clientSocketId,
        device_socket_id: input.deviceSocketId,
        server_instance_id: input.serverInstanceId,
        started_at: input.startedAt
      }
      await options.redis.set(relayKey(input.connectionId), JSON.stringify(route), 'EX', options.ttlSeconds)
    },

    async getRoute(connectionId: string): Promise<RelayRoute | null> {
      const value = await options.redis.get(relayKey(connectionId))
      return value ? (JSON.parse(value) as RelayRoute) : null
    },

    async deleteRoute(connectionId: string) {
      await options.redis.del(relayKey(connectionId))
    }
  }
}
```

- [ ] **Step 5: Run relay route tests and build**

Run:

```bash
cd remote-server
npm test -- relay-route.service.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/modules/relay/relay.schemas.ts remote-server/src/modules/relay/relay-route.service.ts remote-server/tests/relay-route.service.test.ts
git commit -m "feat: 新增远程 relay 路由状态" -m "修改内容：新增 relay 绑定参数、密文帧 schema 和 Redis relay 路由状态读写删除服务。" -m "修改原因：WebRTC 不可用时需要通过服务端按连接 ID 转发端到端加密帧。"
```

## Task 2: Relay Socket Registry

**Files:**
- Create: `remote-server/src/modules/relay/relay-socket-registry.ts`
- Test: `remote-server/tests/relay-socket-registry.test.ts`

- [ ] **Step 1: Write failing registry tests**

Create `remote-server/tests/relay-socket-registry.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { createRelaySocketRegistry } from '../src/modules/relay/relay-socket-registry.js'

describe('relay socket registry', () => {
  it('forwards ciphertext frame to the opposite side', () => {
    const registry = createRelaySocketRegistry()
    const clientSend = vi.fn()
    const deviceSend = vi.fn()

    registry.add({ connectionId: 'conn_1', side: 'client', socketId: 'sock_client', socket: { send: clientSend, close: vi.fn() } })
    registry.add({ connectionId: 'conn_1', side: 'device', socketId: 'sock_device', socket: { send: deviceSend, close: vi.fn() } })

    expect(registry.forward('conn_1', 'client', { type: 'relay.frame', ciphertext: 'abc' })).toBe(true)
    expect(deviceSend).toHaveBeenCalledWith(JSON.stringify({ type: 'relay.frame', ciphertext: 'abc' }))
    expect(clientSend).not.toHaveBeenCalled()
  })

  it('tracks monotonic sequence per side', () => {
    const registry = createRelaySocketRegistry()

    expect(registry.acceptSeq('conn_1', 'client', 1)).toBe(true)
    expect(registry.acceptSeq('conn_1', 'client', 2)).toBe(true)
    expect(registry.acceptSeq('conn_1', 'client', 2)).toBe(false)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- relay-socket-registry.test.ts
```

Expected: FAIL because relay socket registry does not exist.

- [ ] **Step 3: Implement registry**

Create `remote-server/src/modules/relay/relay-socket-registry.ts`:

```ts
import type { RelaySide } from './relay.schemas.js'

export type RelaySocket = {
  send(data: string): void
  close(code: number, reason: string): void
}

export type RelaySocketEntry = {
  connectionId: string
  side: RelaySide
  socketId: string
  socket: RelaySocket
}

function sideKey(connectionId: string, side: RelaySide) {
  return `${connectionId}:${side}`
}

function oppositeSide(side: RelaySide): RelaySide {
  return side === 'client' ? 'device' : 'client'
}

export function createRelaySocketRegistry() {
  const sockets = new Map<string, RelaySocketEntry>()
  const lastSeq = new Map<string, number>()

  return {
    add(entry: RelaySocketEntry) {
      sockets.set(sideKey(entry.connectionId, entry.side), entry)
    },

    remove(connectionId: string, side: RelaySide) {
      sockets.delete(sideKey(connectionId, side))
    },

    getSocketId(connectionId: string, side: RelaySide) {
      return sockets.get(sideKey(connectionId, side))?.socketId ?? null
    },

    forward(connectionId: string, fromSide: RelaySide, message: object) {
      const target = sockets.get(sideKey(connectionId, oppositeSide(fromSide)))
      if (!target) return false
      target.socket.send(JSON.stringify(message))
      return true
    },

    acceptSeq(connectionId: string, side: RelaySide, seq: number) {
      const key = sideKey(connectionId, side)
      const previous = lastSeq.get(key) ?? 0
      if (seq <= previous) return false
      lastSeq.set(key, seq)
      return true
    },

    closeConnection(connectionId: string, code: number, reason: string) {
      for (const side of ['client', 'device'] as const) {
        const entry = sockets.get(sideKey(connectionId, side))
        if (entry) {
          entry.socket.close(code, reason)
          sockets.delete(sideKey(connectionId, side))
        }
      }
    }
  }
}

export type RelaySocketRegistry = ReturnType<typeof createRelaySocketRegistry>
```

- [ ] **Step 4: Run registry tests and build**

Run:

```bash
cd remote-server
npm test -- relay-socket-registry.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add remote-server/src/modules/relay/relay-socket-registry.ts remote-server/tests/relay-socket-registry.test.ts
git commit -m "feat: 新增远程 relay socket 注册表" -m "修改内容：新增 relay socket 注册、对端转发、序号校验和连接关闭能力。" -m "修改原因：服务端 relay 需要在同一连接的 Web 客户端和设备端之间转发密文帧。"
```

## Task 3: Relay Socket Bind And Auth Helpers

**Files:**
- Create: `remote-server/src/ws/relay-socket.ts`
- Test: `remote-server/tests/relay-socket.test.ts`

- [ ] **Step 1: Write failing bind tests**

Create `remote-server/tests/relay-socket.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { bindRelaySocket } from '../src/ws/relay-socket.js'
import { createHash } from '../src/shared/crypto.js'

describe('/ws/relay bind', () => {
  it('binds when connection token and user/device ownership match', async () => {
    const tokenHash = createHash('cnt_valid_token_with_enough_length_123456', 'pepper')
    const result = await bindRelaySocket({
      query: {
        connection_id: 'conn_1',
        connection_token: 'cnt_valid_token_with_enough_length_123456',
        side: 'client'
      },
      actor: { kind: 'client', userId: 'usr_1' },
      tokenPepper: 'pepper',
      state: {
        async get() {
          return {
            connection_id: 'conn_1',
            user_id: 'usr_1',
            device_id: 'dev_1',
            client_id: 'web_1',
            token_hash: tokenHash,
            status: 'signaling',
            created_at: '2026-06-28T00:00:00.000Z',
            expires_at: '2099-01-01T00:00:00.000Z'
          }
        }
      }
    })

    expect(result).toEqual({
      ok: true,
      binding: {
        connectionId: 'conn_1',
        side: 'client',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1'
      }
    })
  })

  it('rejects mismatched token', async () => {
    const result = await bindRelaySocket({
      query: {
        connection_id: 'conn_1',
        connection_token: 'cnt_wrong_token_with_enough_length_123456',
        side: 'client'
      },
      actor: { kind: 'client', userId: 'usr_1' },
      tokenPepper: 'pepper',
      state: {
        async get() {
          return {
            connection_id: 'conn_1',
            user_id: 'usr_1',
            device_id: 'dev_1',
            client_id: 'web_1',
            token_hash: 'bad_hash',
            status: 'signaling',
            created_at: '2026-06-28T00:00:00.000Z',
            expires_at: '2099-01-01T00:00:00.000Z'
          }
        }
      }
    })

    expect(result).toEqual({ ok: false, code: 220403, message: '连接权限不足' })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- relay-socket.test.ts
```

Expected: FAIL because `relay-socket.ts` does not exist.

- [ ] **Step 3: Implement bind helper**

Create `remote-server/src/ws/relay-socket.ts`:

```ts
import type { FastifyInstance, FastifyRequest } from 'fastify'
import websocket from '@fastify/websocket'
import { loadConfigFromEnv } from '../config.js'
import { createDb } from '../db/client.js'
import { createRedis } from '../redis/client.js'
import { requireAuth } from '../modules/auth/auth.middleware.js'
import { createConnectionStateService, type ConnectionState } from '../modules/connections/connection-state.service.js'
import { createConnectionTokenService } from '../modules/connections/connection-token.service.js'
import { createDeviceTokenService } from '../modules/devices/device-token.service.js'
import { createDevicesRepository } from '../modules/devices/devices.repository.js'
import { relayBindSchema, relayFrameSchema, type RelaySide } from '../modules/relay/relay.schemas.js'
import { createRelayRouteService } from '../modules/relay/relay-route.service.js'
import { createRelaySocketRegistry, type RelaySocketRegistry } from '../modules/relay/relay-socket-registry.js'

export type RelayActor = { kind: 'client'; userId: string } | { kind: 'device'; deviceId: string }

export type RelayBinding = {
  connectionId: string
  side: RelaySide
  userId: string
  deviceId: string
  clientId: string
}

export async function bindRelaySocket(input: {
  query: unknown
  actor: RelayActor
  tokenPepper: string
  state: { get(connectionId: string): Promise<ConnectionState | null> }
}): Promise<{ ok: true; binding: RelayBinding } | { ok: false; code: number; message: string }> {
  const parsed = relayBindSchema.safeParse(input.query)
  if (!parsed.success) return { ok: false, code: 100101, message: 'relay 连接参数无效' }

  const state = await input.state.get(parsed.data.connection_id)
  if (!state) return { ok: false, code: 220401, message: '连接不存在' }
  if (new Date(state.expires_at).getTime() <= Date.now()) return { ok: false, code: 220402, message: '连接已过期' }

  const tokenService = createConnectionTokenService({ tokenPepper: input.tokenPepper })
  if (!tokenService.verify(parsed.data.connection_token, state.token_hash)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }

  if (parsed.data.side === 'client' && (input.actor.kind !== 'client' || input.actor.userId !== state.user_id)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }
  if (parsed.data.side === 'device' && (input.actor.kind !== 'device' || input.actor.deviceId !== state.device_id)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }

  return {
    ok: true,
    binding: {
      connectionId: state.connection_id,
      side: parsed.data.side,
      userId: state.user_id,
      deviceId: state.device_id,
      clientId: state.client_id
    }
  }
}
```

- [ ] **Step 4: Run bind tests and build**

Run:

```bash
cd remote-server
npm test -- relay-socket.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add remote-server/src/ws/relay-socket.ts remote-server/tests/relay-socket.test.ts
git commit -m "feat: 新增远程 relay 连接绑定" -m "修改内容：新增 /ws/relay connection token 校验、客户端和设备端归属校验的绑定 helper。" -m "修改原因：relay fallback 必须确保只有当前连接的 Web 客户端和目标设备能加入转发通道。"
```

## Task 4: Relay Frame Forwarding

**Files:**
- Modify: `remote-server/src/ws/relay-socket.ts`
- Test: `remote-server/tests/relay-socket.test.ts`

- [ ] **Step 1: Extend relay frame tests**

Append to `remote-server/tests/relay-socket.test.ts`:

```ts
import { forwardRelayFrame } from '../src/ws/relay-socket.js'

describe('/ws/relay frame forwarding', () => {
  it('forwards ciphertext frame without inspecting payload', async () => {
    const forwarded: object[] = []
    const result = await forwardRelayFrame({
      raw: JSON.stringify({
        version: 1,
        type: 'relay.frame',
        id: 'msg_1',
        connection_id: 'conn_1',
        seq: 1,
        ciphertext: 'eyJlbmNyeXB0ZWQiOiJvcGFxdWUifQ=='
      }),
      binding: {
        connectionId: 'conn_1',
        side: 'client',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1'
      },
      registry: {
        acceptSeq() { return true },
        forward(_connectionId: string, _side: 'client' | 'device', message: object) {
          forwarded.push(message)
          return true
        }
      }
    })

    expect(result).toEqual({ ok: true })
    expect(forwarded[0]).toMatchObject({
      type: 'relay.frame',
      connection_id: 'conn_1',
      ciphertext: 'eyJlbmNyeXB0ZWQiOiJvcGFxdWUifQ=='
    })
  })

  it('rejects repeated sequence numbers', async () => {
    const result = await forwardRelayFrame({
      raw: JSON.stringify({
        version: 1,
        type: 'relay.frame',
        id: 'msg_1',
        connection_id: 'conn_1',
        seq: 1,
        ciphertext: 'abc'
      }),
      binding: {
        connectionId: 'conn_1',
        side: 'client',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1'
      },
      registry: {
        acceptSeq() { return false },
        forward() { return true }
      }
    })

    expect(result).toEqual({ ok: false, code: 220403, message: 'relay 帧序号无效' })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- relay-socket.test.ts
```

Expected: FAIL because `forwardRelayFrame` is not implemented.

- [ ] **Step 3: Implement frame forwarding helper**

Append to `remote-server/src/ws/relay-socket.ts`:

```ts
export async function forwardRelayFrame(input: {
  raw: string
  binding: RelayBinding
  registry: {
    acceptSeq(connectionId: string, side: RelaySide, seq: number): boolean
    forward(connectionId: string, fromSide: RelaySide, message: object): boolean
  }
}): Promise<{ ok: true } | { ok: false; code: number; message: string }> {
  const frame = relayFrameSchema.parse(JSON.parse(input.raw))
  if (frame.connection_id !== input.binding.connectionId) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }
  if (!input.registry.acceptSeq(input.binding.connectionId, input.binding.side, frame.seq)) {
    return { ok: false, code: 220403, message: 'relay 帧序号无效' }
  }

  const sent = input.registry.forward(input.binding.connectionId, input.binding.side, frame)
  if (!sent) return { ok: false, code: 220404, message: '远程设备不可连接' }
  return { ok: true }
}
```

- [ ] **Step 4: Run relay frame tests and build**

Run:

```bash
cd remote-server
npm test -- relay-socket.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 5: Commit**

```bash
git add remote-server/src/ws/relay-socket.ts remote-server/tests/relay-socket.test.ts
git commit -m "feat: 新增远程 relay 密文帧转发" -m "修改内容：新增 relay.frame 校验、连接归属校验、序号校验和对端转发逻辑。" -m "修改原因：WebRTC 不可用时需要通过服务端转发端到端加密后的 RPC 帧。"
```

## Task 5: Register /ws/relay Route

**Files:**
- Modify: `remote-server/src/ws/relay-socket.ts`
- Modify: `remote-server/src/app.ts`
- Test: `remote-server/tests/relay-socket.test.ts`

- [ ] **Step 1: Extend route registration tests**

Append to `remote-server/tests/relay-socket.test.ts`:

```ts
describe('/ws/relay route wiring', () => {
  it('keeps relay close messages code-shaped', () => {
    const reason = JSON.stringify({ code: 220403, message: '连接权限不足' })
    expect(JSON.parse(reason)).toEqual({ code: 220403, message: '连接权限不足' })
  })
})
```

- [ ] **Step 2: Run tests**

Run:

```bash
cd remote-server
npm test -- relay-socket.test.ts
```

Expected: PASS.

- [ ] **Step 3: Implement route registration**

Append to `remote-server/src/ws/relay-socket.ts`:

```ts
export async function registerRelaySocket(app: FastifyInstance, registry: RelaySocketRegistry = createRelaySocketRegistry()) {
  await app.register(websocket)

  const config = loadConfigFromEnv()
  const redis = createRedis(config.redisUrl)
  const { db } = createDb(config.databaseUrl)
  const state = createConnectionStateService({
    redis,
    ttlSeconds: config.connectionTokenTtlSeconds
  })
  const route = createRelayRouteService({
    redis,
    ttlSeconds: config.connectionTokenTtlSeconds
  })
  const devicesRepo = createDevicesRepository(db)
  const deviceTokenService = createDeviceTokenService({
    repo: devicesRepo,
    tokenPepper: config.tokenPepper
  })
  const serverInstanceId = `srv_${process.pid}`

  app.get('/ws/relay', { websocket: true }, async (socket, request) => {
    const parsedQuery = relayBindSchema.safeParse(request.query)
    if (!parsedQuery.success) {
      socket.close(4002, JSON.stringify({ code: 100101, message: 'relay 连接参数无效' }))
      return
    }

    const actor = parsedQuery.data.side === 'client'
      ? await resolveClientActor(request, config.jwtPublicKey)
      : await resolveDeviceActor(request, deviceTokenService)
    if (!actor.ok) {
      socket.close(4001, JSON.stringify({ code: actor.code, message: actor.message }))
      return
    }

    const bound = await bindRelaySocket({
      query: request.query,
      actor: actor.actor,
      tokenPepper: config.tokenPepper,
      state
    })
    if (!bound.ok) {
      socket.close(4003, JSON.stringify({ code: bound.code, message: bound.message }))
      return
    }

    const socketId = `relay_${Date.now()}_${Math.random().toString(36).slice(2)}`
    registry.add({
      connectionId: bound.binding.connectionId,
      side: bound.binding.side,
      socketId,
      socket
    })
    await route.setRoute({
      connectionId: bound.binding.connectionId,
      clientSocketId: bound.binding.side === 'client' ? socketId : registry.getSocketId(bound.binding.connectionId, 'client'),
      deviceSocketId: bound.binding.side === 'device' ? socketId : registry.getSocketId(bound.binding.connectionId, 'device'),
      serverInstanceId,
      startedAt: new Date().toISOString()
    })

    socket.on('message', async (raw) => {
      try {
        const result = await forwardRelayFrame({
          raw: raw.toString(),
          binding: bound.binding,
          registry
        })
        if (!result.ok) socket.close(4004, JSON.stringify({ code: result.code, message: result.message }))
      } catch {
        socket.close(4002, JSON.stringify({ code: 100101, message: 'relay 帧格式无效' }))
      }
    })

    socket.on('close', async () => {
      registry.remove(bound.binding.connectionId, bound.binding.side)
      registry.closeConnection(bound.binding.connectionId, 4000, 'relay_peer_closed')
      await route.deleteRoute(bound.binding.connectionId)
    })
  })
}
```

Add these helper functions in the same file:

```ts
async function resolveClientActor(request: FastifyRequest, jwtPublicKey: string) {
  const auth = await requireAuth(request, jwtPublicKey)
  return auth.ok
    ? { ok: true as const, actor: { kind: 'client' as const, userId: auth.auth.userId } }
    : { ok: false as const, code: 200001, message: '未登录' }
}

async function resolveDeviceActor(request: FastifyRequest, deviceTokenService: ReturnType<typeof createDeviceTokenService>) {
  const auth = await deviceTokenService.authenticate(request.headers.authorization as string | undefined)
  return auth.ok
    ? { ok: true as const, actor: { kind: 'device' as const, deviceId: auth.device.id } }
    : { ok: false as const, code: auth.code, message: auth.message }
}
```

- [ ] **Step 4: Register relay socket in app**

Update `remote-server/src/app.ts`:

```ts
import { registerRelaySocket } from './ws/relay-socket.js'
```

Extend `AppDeps`:

```ts
  registerRelaySocket?: (app: FastifyInstance) => Promise<void>
```

Register after `/ws/client`:

```ts
  void (deps.registerRelaySocket ?? registerRelaySocket)(app)
```

- [ ] **Step 5: Run tests and build**

Run:

```bash
cd remote-server
npm test -- relay-socket.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/ws/relay-socket.ts remote-server/src/app.ts remote-server/tests/relay-socket.test.ts
git commit -m "feat: 注册远程 relay WebSocket" -m "修改内容：新增 /ws/relay 路由注册、client/device 身份解析、relay 路由状态写入和断开清理。" -m "修改原因：远程控制需要在 WebRTC 不可用时具备服务端密文转发 fallback。"
```

## Task 6: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-5.

- [ ] **Step 1: Run full checks**

Run:

```bash
cd remote-server
npm run check
```

Expected: TypeScript build passes and all Vitest tests pass.

- [ ] **Step 2: Verify relay stores no plaintext or ciphertext persistence**

Run:

```bash
rg -n "ciphertext" remote-server/src/db remote-server/src/modules remote-server/src/ws
```

Expected: matches appear in relay frame schema and forwarding code only; no Drizzle schema or repository writes `ciphertext` to PostgreSQL.

- [ ] **Step 3: Verify WebSocket code does not use HTTP envelope**

Run:

```bash
rg -n "apiSuccess|apiFailure" remote-server/src/ws
```

Expected: no output.

- [ ] **Step 4: Verify relay route has no host-facing port changes**

Run:

```bash
rg -n "3000|5173|8080|5432:5432|6379:6379|80:80|443:443" remote-server
```

Expected: no newly introduced host-facing default port mapping. Matches inside tests or blocked-port scans require manual classification before commit.

- [ ] **Step 5: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

- [ ] **Step 6: Record milestone result**

Add this note to the implementation issue or PR description:

```text
Remote server relay slice complete:
- /ws/relay binding for client and device sides
- connection token verification
- Redis relay route state
- ciphertext-only relay.frame validation and forwarding
- per-side relay sequence monotonicity
- relay route cleanup on close

Verification:
- cd remote-server && npm run check
- rg ciphertext persistence scan
- rg WebSocket envelope scan
- rg default host port scan
```

Do not mark remote control complete after this milestone. E2EE RPC session handshake, RPC method routing, local RemoteAgent implementation, and Web console remote UI are still separate milestones.

## Self-Review

Spec coverage in this plan:

- `/ws/relay` route: covered by Task 5.
- `connection_id + connection_token` relay auth: covered by Tasks 3 and 5.
- relay frame structure with `ciphertext`: covered by Tasks 1 and 4.
- service forwards ciphertext without parsing plaintext: covered by Task 4 and Task 6 scan.
- Redis `relay:{connection_id}` route state: covered by Task 1 and Task 5.
- WebSocket messages avoid HTTP envelope: covered by Task 6.
- No host-facing port changes: covered by Task 6.

Known follow-up plans:

- Implement E2EE RPC session handshake and key agreement.
- Implement local RemoteAgent transport selection between WebRTC and relay.
- Implement remote RPC method routing and permission guard.
- Implement Web console remote session UI using the finalized transport.
