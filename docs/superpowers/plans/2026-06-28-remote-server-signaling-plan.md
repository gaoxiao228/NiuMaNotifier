# Remote Server Connection Signaling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the remote server slice that lets a logged-in Web client create a remote connection, receive a short-lived connection token, open `/ws/client`, and exchange WebRTC signaling messages with the online device socket.

**Architecture:** This plan builds on the auth, device presence, and `/ws/device` slices. HTTP routes create durable connection rows and short-lived Redis negotiation state; WebSocket client sockets bind to a specific connection with `Bearer <access_token>` plus `connection_id` and `connection_token`; signaling messages are forwarded between the client socket and the already-connected device socket. Relay ciphertext forwarding is left for the next plan.

**Tech Stack:** Node.js LTS, TypeScript, Fastify, `@fastify/websocket`, Drizzle ORM, PostgreSQL, Redis/ioredis, Zod, Vitest.

---

## Scope Check

This plan covers:

- `POST /api/v1/connections/create`
- `GET /api/v1/connections/ice-config`
- connection token generation and hashed storage/verification.
- Redis `connection:{connection_id}` negotiation state with TTL.
- device socket registry send support.
- `/ws/client` authentication and connection binding.
- WebRTC signaling message schemas: `signal.offer`, `signal.answer`, `signal.ice_candidate`, `signal.cancel`.
- forwarding signaling messages between Web client and RemoteAgent device socket.

This plan does not cover:

- `/ws/relay` ciphertext frames.
- WebRTC media/datachannel implementation in browser or RemoteAgent.
- E2EE RPC handshake and payload encryption.
- Connection lifecycle callbacks from RemoteAgent after WebRTC succeeds.
- Multi-instance socket routing via Redis pub/sub. This plan supports one server instance, matching the current Docker Compose MVP.

## API Standard Notes

HTTP routes follow the user-defined backend API standard:

- Business failures return HTTP `200 + non-zero code`.
- `device_id`, `client_id`, and `transport_preference` are POST body fields.
- `GET /api/v1/connections/ice-config` uses query-free GET because it only returns current server config.
- WebSocket messages do not use HTTP API envelope, but every message has `version`, `type`, `id`, and `data`.

## File Structure

Create:

- `remote-server/src/modules/connections/connections.schemas.ts` - HTTP and WebSocket signaling schemas.
- `remote-server/src/modules/connections/connection-token.service.ts` - opaque connection token creation and verification.
- `remote-server/src/modules/connections/connection-state.service.ts` - Redis `connection:{id}` state read/write/delete.
- `remote-server/src/modules/connections/connections.repository.ts` - Drizzle connection persistence.
- `remote-server/src/modules/connections/connections.service.ts` - create connection and ICE config business rules.
- `remote-server/src/modules/connections/connections.routes.ts` - HTTP routes.
- `remote-server/src/ws/client-socket.ts` - `/ws/client` lifecycle and signaling forwarding.
- `remote-server/tests/connection-token.service.test.ts`
- `remote-server/tests/connection-state.service.test.ts`
- `remote-server/tests/connections.service.test.ts`
- `remote-server/tests/connections.routes.test.ts`
- `remote-server/tests/client-socket.test.ts`

Modify:

- `remote-server/src/shared/errors.ts` - use existing connection error codes from the foundation plan.
- `remote-server/src/config.ts` - expose connection TTL and ICE/TURN config parsing already introduced in foundation.
- `remote-server/src/app.ts` - register connection routes and `/ws/client`.
- `remote-server/src/modules/devices/device-socket-registry.ts` - add `sendToDevice`.
- `remote-server/src/ws/ws-message.schemas.ts` - export shared signaling message schemas or re-export from connections schemas.

## Task 1: Connection Token And Redis State

**Files:**
- Create: `remote-server/src/modules/connections/connection-token.service.ts`
- Create: `remote-server/src/modules/connections/connection-state.service.ts`
- Test: `remote-server/tests/connection-token.service.test.ts`
- Test: `remote-server/tests/connection-state.service.test.ts`

- [ ] **Step 1: Write failing connection-token tests**

Create `remote-server/tests/connection-token.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { createConnectionTokenService } from '../src/modules/connections/connection-token.service.js'

describe('connection token service', () => {
  it('creates opaque connection tokens and verifies hashes', () => {
    const service = createConnectionTokenService({ tokenPepper: 'pepper' })
    const issued = service.issue()

    expect(issued.token).toMatch(/^cnt_[A-Za-z0-9_-]{43,}$/)
    expect(issued.tokenHash).toHaveLength(64)
    expect(service.verify(issued.token, issued.tokenHash)).toBe(true)
    expect(service.verify('cnt_wrong', issued.tokenHash)).toBe(false)
  })
})
```

- [ ] **Step 2: Write failing connection-state tests**

Create `remote-server/tests/connection-state.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { createConnectionStateService, type ConnectionStateRedis } from '../src/modules/connections/connection-state.service.js'

function createFakeRedis(): ConnectionStateRedis {
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

describe('connection state service', () => {
  it('stores and reads short-lived negotiation state', async () => {
    const service = createConnectionStateService({ redis: createFakeRedis(), ttlSeconds: 120 })
    await service.setPending({
      connectionId: 'conn_1',
      userId: 'usr_1',
      deviceId: 'dev_1',
      clientId: 'web_1',
      tokenHash: 'hash',
      status: 'signaling',
      createdAt: '2026-06-28T00:00:00.000Z',
      expiresAt: '2026-06-28T00:02:00.000Z'
    })

    await expect(service.get('conn_1')).resolves.toMatchObject({
      connection_id: 'conn_1',
      user_id: 'usr_1',
      device_id: 'dev_1',
      client_id: 'web_1',
      status: 'signaling'
    })
  })
})
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cd remote-server
npm test -- connection-token.service.test.ts connection-state.service.test.ts
```

Expected: FAIL because connection token and state services do not exist.

- [ ] **Step 4: Implement connection token service**

Create `remote-server/src/modules/connections/connection-token.service.ts`:

```ts
import { createHash, createRandomToken } from '../../shared/crypto.js'

export function createConnectionTokenService(options: { tokenPepper: string }) {
  return {
    issue() {
      const token = createRandomToken('cnt')
      return {
        token,
        tokenHash: createHash(token, options.tokenPepper)
      }
    },
    verify(token: string, expectedHash: string) {
      return createHash(token, options.tokenPepper) === expectedHash
    }
  }
}
```

- [ ] **Step 5: Implement connection state service**

Create `remote-server/src/modules/connections/connection-state.service.ts`:

```ts
export type ConnectionState = {
  connection_id: string
  user_id: string
  device_id: string
  client_id: string
  token_hash: string
  status: 'pending' | 'signaling' | 'connected' | 'closed' | 'expired' | 'failed'
  created_at: string
  expires_at: string
}

export type SetConnectionStateInput = {
  connectionId: string
  userId: string
  deviceId: string
  clientId: string
  tokenHash: string
  status: ConnectionState['status']
  createdAt: string
  expiresAt: string
}

export type ConnectionStateRedis = {
  set(key: string, value: string, mode: 'EX', ttlSeconds: number): Promise<unknown>
  get(key: string): Promise<string | null>
  del(key: string): Promise<unknown>
}

function connectionKey(connectionId: string) {
  return `connection:${connectionId}`
}

export function createConnectionStateService(options: { redis: ConnectionStateRedis; ttlSeconds: number }) {
  return {
    async setPending(input: SetConnectionStateInput) {
      const state: ConnectionState = {
        connection_id: input.connectionId,
        user_id: input.userId,
        device_id: input.deviceId,
        client_id: input.clientId,
        token_hash: input.tokenHash,
        status: input.status,
        created_at: input.createdAt,
        expires_at: input.expiresAt
      }
      await options.redis.set(connectionKey(input.connectionId), JSON.stringify(state), 'EX', options.ttlSeconds)
    },

    async get(connectionId: string): Promise<ConnectionState | null> {
      const value = await options.redis.get(connectionKey(connectionId))
      return value ? (JSON.parse(value) as ConnectionState) : null
    },

    async delete(connectionId: string) {
      await options.redis.del(connectionKey(connectionId))
    }
  }
}
```

- [ ] **Step 6: Run tests and build**

Run:

```bash
cd remote-server
npm test -- connection-token.service.test.ts connection-state.service.test.ts
npm run build
```

Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/connections/connection-token.service.ts remote-server/src/modules/connections/connection-state.service.ts remote-server/tests/connection-token.service.test.ts remote-server/tests/connection-state.service.test.ts
git commit -m "feat: 新增远程连接令牌和协商状态" -m "修改内容：新增 connection token 签发校验和 Redis connection 协商状态读写删除服务。" -m "修改原因：Web 控制台创建远程连接后需要短期令牌和可过期的信令状态。"
```

## Task 2: Connection Schemas, Repository, And Service

**Files:**
- Create: `remote-server/src/modules/connections/connections.schemas.ts`
- Create: `remote-server/src/modules/connections/connections.repository.ts`
- Create: `remote-server/src/modules/connections/connections.service.ts`
- Test: `remote-server/tests/connections.service.test.ts`

- [ ] **Step 1: Write failing connection service tests**

Create `remote-server/tests/connections.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { ErrorCode } from '../src/shared/errors.js'
import { createConnectionsService, type ConnectionsRepository } from '../src/modules/connections/connections.service.js'

function createRepo(deviceOnline: boolean): ConnectionsRepository {
  return {
    async findDeviceForUser(userId, deviceId) {
      if (userId !== 'usr_1' || deviceId !== 'dev_1') return null
      return { id: 'dev_1', userId: 'usr_1', name: 'NiuMa MacBook', status: 'active' }
    },
    async createConnection(input) {
      return { id: input.id, ...input }
    }
  }
}

describe('connections service', () => {
  it('creates connection for online device', async () => {
    const stateWrites: any[] = []
    const service = createConnectionsService({
      repo: createRepo(true),
      presence: {
        async getPresence() {
          return {
            user_id: 'usr_1',
            device_id: 'dev_1',
            socket_id: 'sock_1',
            server_instance_id: 'srv_1',
            last_seen_at: '2026-06-28T00:00:00.000Z',
            capabilities: {}
          }
        }
      },
      state: {
        async setPending(input) {
          stateWrites.push(input)
        }
      },
      tokenPepper: 'pepper',
      publicUrl: 'https://remote.example.com',
      ttlSeconds: 120
    })

    const result = await service.create({
      userId: 'usr_1',
      deviceId: 'dev_1',
      clientId: 'web_1',
      transportPreference: 'webrtc_first'
    })

    expect(result.ok).toBe(true)
    if (!result.ok) throw new Error('connection create failed')
    expect(result.data.connection_id).toMatch(/^conn_/)
    expect(result.data.connection_token).toMatch(/^cnt_/)
    expect(result.data.signaling_url).toBe('wss://remote.example.com/ws/client')
    expect(stateWrites).toHaveLength(1)
  })

  it('rejects offline device as business failure', async () => {
    const service = createConnectionsService({
      repo: createRepo(false),
      presence: { async getPresence() { return null } },
      state: { async setPending() {} },
      tokenPepper: 'pepper',
      publicUrl: 'https://remote.example.com',
      ttlSeconds: 120
    })

    await expect(service.create({
      userId: 'usr_1',
      deviceId: 'dev_1',
      clientId: 'web_1',
      transportPreference: 'webrtc_first'
    })).resolves.toEqual({
      ok: false,
      code: ErrorCode.DEVICE_OFFLINE,
      message: '设备离线'
    })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- connections.service.test.ts
```

Expected: FAIL because connection schemas, repository, and service do not exist.

- [ ] **Step 3: Implement schemas**

Create `remote-server/src/modules/connections/connections.schemas.ts`:

```ts
import { z } from 'zod'

export const connectionCreateSchema = z.object({
  device_id: z.string().min(1).max(160),
  client_id: z.string().min(1).max(160),
  transport_preference: z.enum(['webrtc_first', 'relay_first', 'relay_only']).default('webrtc_first')
})

export const connectionClientBindSchema = z.object({
  connection_id: z.string().min(1).max(160),
  connection_token: z.string().min(32)
})

export const signalingPayloadSchema = z.object({
  sdp: z.string().optional(),
  candidate: z.unknown().optional()
})

export const clientSignalMessageSchema = z.discriminatedUnion('type', [
  z.object({ version: z.literal(1), id: z.string().min(1), type: z.literal('signal.offer'), data: signalingPayloadSchema }),
  z.object({ version: z.literal(1), id: z.string().min(1), type: z.literal('signal.answer'), data: signalingPayloadSchema }),
  z.object({ version: z.literal(1), id: z.string().min(1), type: z.literal('signal.ice_candidate'), data: signalingPayloadSchema }),
  z.object({ version: z.literal(1), id: z.string().min(1), type: z.literal('signal.cancel'), data: z.object({ reason: z.string().min(1).max(240) }) })
])

export type ConnectionCreateInput = z.infer<typeof connectionCreateSchema>
export type ConnectionClientBindInput = z.infer<typeof connectionClientBindSchema>
export type ClientSignalMessage = z.infer<typeof clientSignalMessageSchema>
```

- [ ] **Step 4: Implement repository**

Create `remote-server/src/modules/connections/connections.repository.ts`:

```ts
import { and, eq } from 'drizzle-orm'
import { devices, remoteConnections } from '../../db/schema.js'
import type { ConnectionsRepository } from './connections.service.js'

export function createConnectionsRepository(db: any): ConnectionsRepository {
  return {
    async findDeviceForUser(userId, deviceId) {
      return (await db
        .select()
        .from(devices)
        .where(and(eq(devices.userId, userId), eq(devices.id, deviceId), eq(devices.status, 'active')))
        .limit(1))[0] ?? null
    },

    async createConnection(input) {
      return (await db.insert(remoteConnections).values(input).returning())[0]
    }
  }
}
```

- [ ] **Step 5: Implement service**

Create `remote-server/src/modules/connections/connections.service.ts`:

```ts
import { ErrorCode, type ErrorCodeValue } from '../../shared/errors.js'
import { createPublicId } from '../../shared/id.js'
import { addSeconds, systemClock, type Clock } from '../../shared/time.js'
import { createConnectionTokenService } from './connection-token.service.js'

export type ConnectionsRepository = {
  findDeviceForUser(userId: string, deviceId: string): Promise<{ id: string; userId: string; name: string; status: string } | null>
  createConnection(input: {
    id: string
    userId: string
    deviceId: string
    clientId: string
    status: string
    transportPreference: string
    transportSelected: string | null
    expiresAt: Date
    createdAt: Date
    connectedAt: Date | null
    closedAt: Date | null
    closeReason: string | null
  }): Promise<{ id: string }>
}

export type ConnectionPresenceReader = {
  getPresence(deviceId: string): Promise<unknown | null>
}

export type ConnectionStateWriter = {
  setPending(input: {
    connectionId: string
    userId: string
    deviceId: string
    clientId: string
    tokenHash: string
    status: 'pending' | 'signaling'
    createdAt: string
    expiresAt: string
  }): Promise<void>
}

export type ConnectionFailure = {
  ok: false
  code: ErrorCodeValue
  message: string
}

function toWebSocketUrl(publicUrl: string, path: string) {
  const url = new URL(publicUrl)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  url.pathname = path
  url.search = ''
  return url.toString()
}

export function createConnectionsService(options: {
  repo: ConnectionsRepository
  presence: ConnectionPresenceReader
  state: ConnectionStateWriter
  tokenPepper: string
  publicUrl: string
  ttlSeconds: number
  clock?: Clock
}) {
  const clock = options.clock ?? systemClock
  const tokenService = createConnectionTokenService({ tokenPepper: options.tokenPepper })

  return {
    async create(input: {
      userId: string
      deviceId: string
      clientId: string
      transportPreference: 'webrtc_first' | 'relay_first' | 'relay_only'
    }) {
      const device = await options.repo.findDeviceForUser(input.userId, input.deviceId)
      if (!device) return { ok: false as const, code: ErrorCode.DEVICE_NOT_FOUND, message: '设备不存在' }

      const presence = await options.presence.getPresence(input.deviceId)
      if (!presence) return { ok: false as const, code: ErrorCode.DEVICE_OFFLINE, message: '设备离线' }

      const now = clock.now()
      const expiresAt = addSeconds(now, options.ttlSeconds)
      const connectionId = createPublicId('conn')
      const issued = tokenService.issue()

      await options.repo.createConnection({
        id: connectionId,
        userId: input.userId,
        deviceId: input.deviceId,
        clientId: input.clientId,
        status: 'signaling',
        transportPreference: input.transportPreference,
        transportSelected: null,
        expiresAt,
        createdAt: now,
        connectedAt: null,
        closedAt: null,
        closeReason: null
      })

      await options.state.setPending({
        connectionId,
        userId: input.userId,
        deviceId: input.deviceId,
        clientId: input.clientId,
        tokenHash: issued.tokenHash,
        status: 'signaling',
        createdAt: now.toISOString(),
        expiresAt: expiresAt.toISOString()
      })

      return {
        ok: true as const,
        data: {
          connection_id: connectionId,
          connection_token: issued.token,
          expires_in: options.ttlSeconds,
          signaling_url: toWebSocketUrl(options.publicUrl, '/ws/client'),
          relay_url: toWebSocketUrl(options.publicUrl, '/ws/relay')
        }
      }
    },

    iceConfig(turn: { enabled: boolean; urls: string[]; username: string; credential: string }) {
      return {
        ice_servers: turn.enabled
          ? [{ urls: turn.urls }, { urls: turn.urls, username: turn.username, credential: turn.credential }]
          : []
      }
    }
  }
}
```

- [ ] **Step 6: Run tests and build**

Run:

```bash
cd remote-server
npm test -- connections.service.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/connections/connections.schemas.ts remote-server/src/modules/connections/connections.repository.ts remote-server/src/modules/connections/connections.service.ts remote-server/tests/connections.service.test.ts
git commit -m "feat: 新增远程连接创建服务" -m "修改内容：新增连接创建参数模型、连接持久化仓储和业务服务，生成 connection token 与信令地址。" -m "修改原因：Web 控制台需要为在线设备创建短期远程连接并进入 WebRTC 信令阶段。"
```

## Task 3: Connection HTTP Routes

**Files:**
- Create: `remote-server/src/modules/connections/connections.routes.ts`
- Modify: `remote-server/src/app.ts`
- Test: `remote-server/tests/connections.routes.test.ts`

- [ ] **Step 1: Write failing route tests**

Create `remote-server/tests/connections.routes.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { apiFailure, apiSuccess } from '../src/shared/response.js'
import { ErrorCode } from '../src/shared/errors.js'

describe('connection routes', () => {
  it('requires bearer token for create', async () => {
    const app = buildApp({
      registerConnectionsRoutes: async (app) => {
        app.post('/api/v1/connections/create', async () => apiFailure(ErrorCode.UNAUTHORIZED, '未登录'))
      }
    })

    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/connections/create',
      payload: { device_id: 'dev_1', client_id: 'web_1', transport_preference: 'webrtc_first' }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(200001)
  })

  it('returns ice config envelope', async () => {
    const app = buildApp({
      registerConnectionsRoutes: async (app) => {
        app.get('/api/v1/connections/ice-config', async () => apiSuccess({ ice_servers: [] }))
      }
    })

    const response = await app.inject({
      method: 'GET',
      url: '/api/v1/connections/ice-config'
    })

    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 0,
      message: 'ok',
      data: { ice_servers: [] }
    })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- connections.routes.test.ts
```

Expected: FAIL because `buildApp` does not support `registerConnectionsRoutes` before this task.

- [ ] **Step 3: Implement routes**

Create `remote-server/src/modules/connections/connections.routes.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { createRedis } from '../../redis/client.js'
import { apiFailure, apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { requireAuth } from '../auth/auth.middleware.js'
import type { DeviceSocketRegistry } from '../devices/device-socket-registry.js'
import { createPresenceService } from '../devices/presence.service.js'
import { createConnectionsRepository } from './connections.repository.js'
import { createConnectionsService } from './connections.service.js'
import { connectionCreateSchema } from './connections.schemas.js'
import { createConnectionStateService } from './connection-state.service.js'

export async function registerConnectionsRoutes(app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) {
  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const redis = createRedis(config.redisUrl)
  const service = createConnectionsService({
    repo: createConnectionsRepository(db),
    presence: createPresenceService({ redis, ttlSeconds: config.devicePresenceTtlSeconds }),
    state: createConnectionStateService({ redis, ttlSeconds: config.connectionTokenTtlSeconds }),
    tokenPepper: config.tokenPepper,
    publicUrl: config.publicUrl,
    ttlSeconds: config.connectionTokenTtlSeconds
  })

  app.post('/api/v1/connections/create', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response

    const parsed = parseBody(connectionCreateSchema, request.body)
    if (!parsed.ok) return parsed.response

    const result = await service.create({
      userId: auth.auth.userId,
      deviceId: parsed.data.device_id,
      clientId: parsed.data.client_id,
      transportPreference: parsed.data.transport_preference
    })
    if (result.ok) {
      deps.registry.sendToDevice(parsed.data.device_id, createConnectionInviteMessage({
        connectionId: result.data.connection_id,
        clientId: parsed.data.client_id,
        transportPreference: parsed.data.transport_preference
      }))
    }
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.get('/api/v1/connections/ice-config', async () => {
    return apiSuccess(service.iceConfig(config.turn))
  })
}

export function createConnectionInviteMessage(input: {
  connectionId: string
  clientId: string
  transportPreference: 'webrtc_first' | 'relay_first' | 'relay_only'
}) {
  return {
    version: 1,
    type: 'connection.invite',
    id: `msg_${input.connectionId}`,
    connection_id: input.connectionId,
    data: {
      client_id: input.clientId,
      transport_preference: input.transportPreference
    }
  }
}
```

- [ ] **Step 4: Register routes in app**

Update `remote-server/src/app.ts`:

```ts
import { registerConnectionsRoutes } from './modules/connections/connections.routes.js'
```

Extend `AppDeps`:

```ts
export type AppDeps = {
  registerAuthRoutes?: (app: FastifyInstance) => Promise<void>
  registerDesktopLoginRoutes?: (app: FastifyInstance) => Promise<void>
  registerDevicesRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerConnectionsRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerDeviceSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
}
```

Register after devices routes:

```ts
  void (deps.registerConnectionsRoutes ?? registerConnectionsRoutes)(app, { registry: deviceSocketRegistry })
```

- [ ] **Step 5: Run route tests and build**

Run:

```bash
cd remote-server
npm test -- connections.routes.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/modules/connections/connections.routes.ts remote-server/src/app.ts remote-server/tests/connections.routes.test.ts
git commit -m "feat: 新增远程连接 HTTP 接口" -m "修改内容：新增创建远程连接和获取 ICE 配置接口，创建成功后向设备发送 connection.invite。" -m "修改原因：Web 控制台需要通过服务端为在线设备创建信令连接入口。"
```

## Task 4: Device Registry Send Support And Signaling Schemas

**Files:**
- Modify: `remote-server/src/modules/devices/device-socket-registry.ts`
- Modify: `remote-server/src/ws/ws-message.schemas.ts`
- Test: `remote-server/tests/client-socket.test.ts`

- [ ] **Step 1: Write failing registry send tests**

Create `remote-server/tests/client-socket.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'
import { clientSignalMessageSchema } from '../src/modules/connections/connections.schemas.js'

describe('client signaling prerequisites', () => {
  it('sends messages to registered device socket', () => {
    const registry = createDeviceSocketRegistry()
    const send = vi.fn()
    registry.add('dev_1', { close: vi.fn(), send })

    expect(registry.sendToDevice('dev_1', { type: 'signal.offer' })).toBe(true)
    expect(send).toHaveBeenCalledWith(JSON.stringify({ type: 'signal.offer' }))
  })

  it('validates signaling messages', () => {
    expect(clientSignalMessageSchema.parse({
      version: 1,
      id: 'msg_1',
      type: 'signal.offer',
      data: { sdp: 'offer-sdp' }
    }).type).toBe('signal.offer')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- client-socket.test.ts
```

Expected: FAIL because registry does not support `sendToDevice` or schemas are not exported.

- [ ] **Step 3: Update socket registry**

Update `remote-server/src/modules/devices/device-socket-registry.ts`:

```ts
export type DeviceSocket = {
  close(code: number, reason: string): void
  send?(data: string): void
}

export function createDeviceSocketRegistry() {
  const sockets = new Map<string, DeviceSocket>()

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
    }
  }
}

export type DeviceSocketRegistry = ReturnType<typeof createDeviceSocketRegistry>
```

- [ ] **Step 4: Re-export signaling schemas**

Update `remote-server/src/ws/ws-message.schemas.ts`:

```ts
export { clientSignalMessageSchema } from '../modules/connections/connections.schemas.js'
```

- [ ] **Step 5: Run tests and build**

Run:

```bash
cd remote-server
npm test -- client-socket.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/modules/devices/device-socket-registry.ts remote-server/src/ws/ws-message.schemas.ts remote-server/tests/client-socket.test.ts
git commit -m "feat: 支持向远程设备转发信令" -m "修改内容：设备 socket 注册表新增 sendToDevice，并导出 Web 客户端信令消息 schema。" -m "修改原因：/ws/client 需要把 WebRTC offer、answer 和 ICE candidate 转发给在线 RemoteAgent。"
```

## Task 5: /ws/client Binding And Signaling Forward

**Files:**
- Create: `remote-server/src/ws/client-socket.ts`
- Modify: `remote-server/src/app.ts`
- Test: `remote-server/tests/client-socket.test.ts`

- [ ] **Step 1: Extend client socket tests**

Append to `remote-server/tests/client-socket.test.ts`:

```ts
import { bindClientConnection, forwardClientSignal } from '../src/ws/client-socket.js'

describe('/ws/client signaling', () => {
  it('binds only when bearer user and connection token match Redis state', async () => {
    const result = await bindClientConnection({
      auth: { userId: 'usr_1', sessionId: 'rft_1', role: 'user' },
      query: { connection_id: 'conn_1', connection_token: 'cnt_token' },
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

    expect(result.ok).toBe(false)
  })

  it('forwards valid signal messages to device socket', async () => {
    const sent: object[] = []
    const result = await forwardClientSignal({
      raw: JSON.stringify({ version: 1, id: 'msg_1', type: 'signal.offer', data: { sdp: 'offer' } }),
      connection: { connectionId: 'conn_1', userId: 'usr_1', deviceId: 'dev_1', clientId: 'web_1' },
      registry: {
        sendToDevice(_deviceId: string, message: object) {
          sent.push(message)
          return true
        }
      }
    })

    expect(result).toEqual({ ok: true })
    expect(sent[0]).toMatchObject({
      version: 1,
      type: 'signal.offer',
      connection_id: 'conn_1'
    })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- client-socket.test.ts
```

Expected: FAIL because `client-socket.ts` does not exist.

- [ ] **Step 3: Implement client socket helpers and route**

Create `remote-server/src/ws/client-socket.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import websocket from '@fastify/websocket'
import { loadConfigFromEnv } from '../config.js'
import { createRedis } from '../redis/client.js'
import { requireAuth, type AuthContext } from '../modules/auth/auth.middleware.js'
import type { DeviceSocketRegistry } from '../modules/devices/device-socket-registry.js'
import { createConnectionStateService, type ConnectionState } from '../modules/connections/connection-state.service.js'
import { createConnectionTokenService } from '../modules/connections/connection-token.service.js'
import { clientSignalMessageSchema, connectionClientBindSchema } from '../modules/connections/connections.schemas.js'

export type BoundClientConnection = {
  connectionId: string
  userId: string
  deviceId: string
  clientId: string
}

export async function bindClientConnection(input: {
  auth: AuthContext
  query: unknown
  tokenPepper: string
  state: { get(connectionId: string): Promise<ConnectionState | null> }
}): Promise<{ ok: true; connection: BoundClientConnection } | { ok: false; code: number; message: string }> {
  const parsed = connectionClientBindSchema.safeParse(input.query)
  if (!parsed.success) return { ok: false, code: 100101, message: '连接参数无效' }

  const state = await input.state.get(parsed.data.connection_id)
  if (!state) return { ok: false, code: 220401, message: '连接不存在' }
  if (state.user_id !== input.auth.userId) return { ok: false, code: 220403, message: '连接权限不足' }
  if (new Date(state.expires_at).getTime() <= Date.now()) return { ok: false, code: 220402, message: '连接已过期' }

  const tokenService = createConnectionTokenService({ tokenPepper: input.tokenPepper })
  if (!tokenService.verify(parsed.data.connection_token, state.token_hash)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }

  return {
    ok: true,
    connection: {
      connectionId: state.connection_id,
      userId: state.user_id,
      deviceId: state.device_id,
      clientId: state.client_id
    }
  }
}

export async function forwardClientSignal(input: {
  raw: string
  connection: BoundClientConnection
  registry: { sendToDevice(deviceId: string, message: object): boolean }
}) {
  const message = clientSignalMessageSchema.parse(JSON.parse(input.raw))
  const sent = input.registry.sendToDevice(input.connection.deviceId, {
    ...message,
    connection_id: input.connection.connectionId,
    client_id: input.connection.clientId
  })
  return sent ? { ok: true as const } : { ok: false as const, code: 210404, message: '设备离线' }
}

export async function registerClientSocket(app: FastifyInstance, registry: DeviceSocketRegistry) {
  await app.register(websocket)

  const config = loadConfigFromEnv()
  const redis = createRedis(config.redisUrl)
  const state = createConnectionStateService({
    redis,
    ttlSeconds: config.connectionTokenTtlSeconds
  })

  app.get('/ws/client', { websocket: true }, async (socket, request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) {
      socket.close(4001, JSON.stringify({ code: 200001, message: '未登录' }))
      return
    }

    const bound = await bindClientConnection({
      auth: auth.auth,
      query: request.query,
      tokenPepper: config.tokenPepper,
      state
    })
    if (!bound.ok) {
      socket.close(4003, JSON.stringify({ code: bound.code, message: bound.message }))
      return
    }

    socket.on('message', async (raw) => {
      try {
        const result = await forwardClientSignal({
          raw: raw.toString(),
          connection: bound.connection,
          registry
        })
        if (!result.ok) socket.close(4004, JSON.stringify({ code: result.code, message: result.message }))
      } catch {
        socket.close(4002, 'invalid_client_signal')
      }
    })
  })
}
```

- [ ] **Step 4: Register client socket in app**

Update `remote-server/src/app.ts`:

```ts
import { registerClientSocket } from './ws/client-socket.js'
```

Extend `AppDeps`:

```ts
  registerClientSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
```

Register after `/ws/device` with the same device socket registry:

```ts
  void (deps.registerClientSocket ?? registerClientSocket)(app, deviceSocketRegistry)
```

- [ ] **Step 5: Run tests and build**

Run:

```bash
cd remote-server
npm test -- client-socket.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/ws/client-socket.ts remote-server/src/app.ts remote-server/tests/client-socket.test.ts
git commit -m "feat: 新增远程客户端信令通道" -m "修改内容：新增 /ws/client 鉴权、connection token 绑定和 WebRTC 信令转发到设备 socket。" -m "修改原因：Web 控制台需要通过服务端与在线 RemoteAgent 完成 WebRTC offer、answer 和 ICE candidate 交换。"
```

## Task 6: Connection Invite Message Regression

**Files:**
- Modify: `remote-server/src/modules/connections/connections.routes.ts`
- Test: `remote-server/tests/connections.routes.test.ts`

- [ ] **Step 1: Extend route tests for device invitation shape**

Append to `remote-server/tests/connections.routes.test.ts`:

```ts
import { createConnectionInviteMessage } from '../src/modules/connections/connections.routes.js'

describe('connection create device notification', () => {
  it('uses a device invitation message shape', () => {
    const message = createConnectionInviteMessage({
      connectionId: 'conn_1',
      clientId: 'web_1',
      transportPreference: 'webrtc_first'
    })

    expect(message.type).toBe('connection.invite')
    expect(message.connection_id).toBe('conn_1')
    expect(message.data.transport_preference).toBe('webrtc_first')
  })
})
```

- [ ] **Step 2: Run test**

Run:

```bash
cd remote-server
npm test -- connections.routes.test.ts
```

Expected: PASS.

- [ ] **Step 3: Run route tests and build**

Run:

```bash
cd remote-server
npm test -- connections.routes.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 4: Commit**

```bash
git add remote-server/src/modules/connections/connections.routes.ts remote-server/tests/connections.routes.test.ts
git commit -m "test: 补充远程连接邀请消息测试" -m "修改内容：补充 connection.invite 消息形状回归测试。" -m "修改原因：RemoteAgent 依赖该信令消息进入远程连接准备状态，需要锁定协议形状。"
```

## Task 7: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-6.

- [ ] **Step 1: Run full checks**

Run:

```bash
cd remote-server
npm run check
```

Expected: TypeScript build passes and all Vitest tests pass.

- [ ] **Step 2: Verify HTTP APIs have no dynamic path params**

Run:

```bash
rg -n "app\\.(get|post|put|delete)\\('/api/[^']*:" remote-server/src/modules remote-server/src/ws
```

Expected: no output.

- [ ] **Step 3: Verify WebSocket code does not use HTTP envelope**

Run:

```bash
rg -n "apiSuccess|apiFailure" remote-server/src/ws
```

Expected: no output.

- [ ] **Step 4: Verify relay is not implemented in this milestone**

Run:

```bash
rg -n "relay\\.frame|registerRelay" remote-server/src
```

Expected: no output.

- [ ] **Step 5: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

- [ ] **Step 6: Record milestone result**

Add this note to the implementation issue or PR description:

```text
Remote server connection signaling slice complete:
- POST /api/v1/connections/create
- GET /api/v1/connections/ice-config
- connection token hash verification
- Redis connection negotiation state
- /ws/client binding
- WebRTC offer/answer/ICE/cancel signaling forwarded to /ws/device
- connection.invite notification to RemoteAgent

Verification:
- cd remote-server && npm run check
- rg dynamic API path scan
- rg WebSocket envelope scan
- rg relay implementation boundary scan
```

Do not mark remote transport complete after this milestone. `/ws/relay`, relay route state, ciphertext frame forwarding, WebRTC success/failure lifecycle updates, and E2EE RPC session encryption remain separate milestones.

## Self-Review

Spec coverage in this plan:

- `POST /api/v1/connections/create`: covered by Tasks 2, 3, and 6.
- `GET /api/v1/connections/ice-config`: covered by Tasks 2 and 3.
- `connection:{connection_id}` Redis state with TTL: covered by Task 1.
- connection token hash storage and verification: covered by Tasks 1 and 5.
- `/ws/client` Bearer auth plus connection token binding: covered by Task 5.
- Signaling messages forwarded between Web client and device socket: covered by Tasks 4, 5, and 6.
- WebSocket messages do not use HTTP envelope: covered by Task 7.
- Relay remains out of scope: covered by Task 7.

Known follow-up plans:

- Implement `/ws/relay` ciphertext forwarding.
- Add connection status updates for WebRTC connected, failed, closed, and expired.
- Add Redis pub/sub or another routing layer for multi-instance socket forwarding.
- Implement E2EE RPC session handshake over WebRTC DataChannel or relay transport.
