# Remote Server Device Presence Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the remote server slice that lets a local RemoteAgent authenticate with a device token, keep `/ws/device` online, publish Redis presence, and expose accurate online status in the device list.

**Architecture:** This plan builds on the remote-server foundation and auth/desktop-login plans. It keeps WebSocket transport in `remote-server/src/ws/`, keeps Redis presence in `modules/devices/presence.service.ts`, and keeps device-token validation in `modules/devices/device-token.service.ts`. It intentionally does not implement `/ws/client`, connection creation, WebRTC signaling, relay frames, or E2EE RPC.

**Tech Stack:** Node.js LTS, TypeScript, Fastify, `@fastify/websocket`, Drizzle ORM, PostgreSQL, Redis/ioredis, Zod, Vitest.

---

## Scope Check

This plan covers:

- `Authorization: Device <device_token>` validation.
- `/ws/device` registration.
- `device.hello` message validation.
- `device.heartbeat` message handling.
- Redis `presence:device:{device_id}` write, refresh, read, and delete.
- Device list merging PostgreSQL records with Redis online state.
- In-process socket registry for closing a connected device when its token is revoked.
- `POST /api/v1/devices/revoke-token` as the first device-token revocation API.

This plan does not cover:

- `/ws/client`.
- `/ws/relay`.
- Connection creation and connection token.
- WebRTC offer/answer/ICE forwarding.
- Remote RPC payload encryption.
- Device unbind and rename UI behavior.
- Multi-instance socket closing through Redis pub/sub. This plan adds the single-instance registry; a later scaling slice adds cross-instance fanout.

## Protocol Notes

WebSocket messages do not use HTTP API envelope. Every message uses:

```json
{
  "version": 1,
  "type": "device.heartbeat",
  "id": "msg_001",
  "data": {}
}
```

HTTP routes in this plan still follow the user-defined backend API standard:

- Business failures return HTTP `200 + non-zero code`.
- Device token validation failures on WebSocket upgrade close the socket with an explicit close code and JSON reason message.
- Device revoke route uses POST body, not a path parameter.

## File Structure

Create:

- `remote-server/src/redis/client.ts` - Redis client factory.
- `remote-server/src/modules/devices/device-token.service.ts` - device token hashing and validation.
- `remote-server/src/modules/devices/presence.service.ts` - Redis presence read/write/delete.
- `remote-server/src/modules/devices/device-socket-registry.ts` - in-process device socket map.
- `remote-server/src/ws/ws-message.schemas.ts` - shared WebSocket message schemas.
- `remote-server/src/ws/device-socket.ts` - `/ws/device` lifecycle.
- `remote-server/tests/device-token.service.test.ts`
- `remote-server/tests/presence.service.test.ts`
- `remote-server/tests/device-socket.test.ts`
- `remote-server/tests/devices.presence.routes.test.ts`

Modify:

- `remote-server/src/app.ts` - register `@fastify/websocket` and `/ws/device`.
- `remote-server/src/modules/devices/devices.repository.ts` - add token lookup, token revoke, last-seen update helpers.
- `remote-server/src/modules/devices/devices.service.ts` - merge Redis presence into list response.
- `remote-server/src/modules/devices/devices.routes.ts` - add `POST /api/v1/devices/revoke-token`.
- `remote-server/src/modules/devices/devices.schemas.ts` - add revoke-token schema.
- `remote-server/src/config.ts` - expose presence TTL and heartbeat timeout config.
- `remote-server/.env.example` - document presence TTL without changing host-facing ports.

## Task 1: Redis Client And Presence Service

**Files:**
- Create: `remote-server/src/redis/client.ts`
- Create: `remote-server/src/modules/devices/presence.service.ts`
- Modify: `remote-server/src/config.ts`
- Modify: `remote-server/.env.example`
- Test: `remote-server/tests/presence.service.test.ts`

- [ ] **Step 1: Write failing presence tests**

Create `remote-server/tests/presence.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { createPresenceService, type PresenceRedis } from '../src/modules/devices/presence.service.js'

function createFakeRedis(): PresenceRedis {
  const values = new Map<string, string>()
  return {
    async set(key, value, mode, ttlSeconds) {
      expect(mode).toBe('EX')
      expect(ttlSeconds).toBe(90)
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

describe('presence service', () => {
  it('writes, reads, and deletes device presence', async () => {
    const service = createPresenceService({
      redis: createFakeRedis(),
      ttlSeconds: 90
    })

    await service.markOnline({
      userId: 'usr_1',
      deviceId: 'dev_1',
      socketId: 'sock_1',
      serverInstanceId: 'srv_1',
      lastSeenAt: '2026-06-28T00:00:00.000Z',
      capabilities: { supports_webrtc: true }
    })

    await expect(service.getPresence('dev_1')).resolves.toMatchObject({
      user_id: 'usr_1',
      device_id: 'dev_1',
      socket_id: 'sock_1'
    })

    await service.markOffline('dev_1')
    await expect(service.getPresence('dev_1')).resolves.toBeNull()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- presence.service.test.ts
```

Expected: FAIL because `presence.service.ts` does not exist.

- [ ] **Step 3: Implement Redis client**

Create `remote-server/src/redis/client.ts`:

```ts
import Redis from 'ioredis'

export function createRedis(redisUrl: string) {
  return new Redis(redisUrl, {
    lazyConnect: true,
    maxRetriesPerRequest: 2
  })
}
```

- [ ] **Step 4: Implement presence service**

Create `remote-server/src/modules/devices/presence.service.ts`:

```ts
export type PresenceRecord = {
  user_id: string
  device_id: string
  socket_id: string
  server_instance_id: string
  last_seen_at: string
  capabilities: unknown
}

export type MarkOnlineInput = {
  userId: string
  deviceId: string
  socketId: string
  serverInstanceId: string
  lastSeenAt: string
  capabilities: unknown
}

export type PresenceRedis = {
  set(key: string, value: string, mode: 'EX', ttlSeconds: number): Promise<unknown>
  get(key: string): Promise<string | null>
  del(key: string): Promise<unknown>
}

function presenceKey(deviceId: string) {
  return `presence:device:${deviceId}`
}

export function createPresenceService(options: { redis: PresenceRedis; ttlSeconds: number }) {
  return {
    async markOnline(input: MarkOnlineInput) {
      const value: PresenceRecord = {
        user_id: input.userId,
        device_id: input.deviceId,
        socket_id: input.socketId,
        server_instance_id: input.serverInstanceId,
        last_seen_at: input.lastSeenAt,
        capabilities: input.capabilities
      }
      await options.redis.set(presenceKey(input.deviceId), JSON.stringify(value), 'EX', options.ttlSeconds)
    },

    async getPresence(deviceId: string): Promise<PresenceRecord | null> {
      const value = await options.redis.get(presenceKey(deviceId))
      return value ? (JSON.parse(value) as PresenceRecord) : null
    },

    async markOffline(deviceId: string) {
      await options.redis.del(presenceKey(deviceId))
    }
  }
}
```

- [ ] **Step 5: Add config values**

Update `remote-server/src/config.ts` env schema:

```ts
  DEVICE_PRESENCE_TTL_SECONDS: z.coerce.number().int().positive().default(90),
  DEVICE_HEARTBEAT_TIMEOUT_SECONDS: z.coerce.number().int().positive().default(45),
```

Update returned config:

```ts
    devicePresenceTtlSeconds: parsed.DEVICE_PRESENCE_TTL_SECONDS,
    deviceHeartbeatTimeoutSeconds: parsed.DEVICE_HEARTBEAT_TIMEOUT_SECONDS,
```

Update `remote-server/.env.example`:

```env
DEVICE_PRESENCE_TTL_SECONDS=90
DEVICE_HEARTBEAT_TIMEOUT_SECONDS=45
```

- [ ] **Step 6: Run presence tests and build**

Run:

```bash
cd remote-server
npm test -- presence.service.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/redis/client.ts remote-server/src/modules/devices/presence.service.ts remote-server/src/config.ts remote-server/.env.example remote-server/tests/presence.service.test.ts
git commit -m "feat: 新增远程设备在线状态服务" -m "修改内容：新增 Redis 客户端、设备 presence 读写删除服务和在线状态 TTL 配置。" -m "修改原因：远程服务端需要判断本机 RemoteAgent 是否在线可用。"
```

## Task 2: Device Token Validation And Repository Methods

**Files:**
- Create: `remote-server/src/modules/devices/device-token.service.ts`
- Modify: `remote-server/src/modules/devices/devices.repository.ts`
- Test: `remote-server/tests/device-token.service.test.ts`

- [ ] **Step 1: Write failing device-token tests**

Create `remote-server/tests/device-token.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { ErrorCode } from '../src/shared/errors.js'
import { createHash } from '../src/shared/crypto.js'
import { createDeviceTokenService, type DeviceTokenRepository } from '../src/modules/devices/device-token.service.js'

function createRepo(): DeviceTokenRepository {
  const tokenHash = createHash('dvt_valid', 'pepper')
  return {
    async findActiveDeviceByTokenHash(inputHash) {
      if (inputHash !== tokenHash) return null
      return {
        id: 'dev_1',
        userId: 'usr_1',
        name: 'NiuMa MacBook',
        status: 'active',
        revokedAt: null,
        capabilityJson: {}
      }
    },
    async updateLastSeen() {}
  }
}

describe('device token service', () => {
  it('accepts valid active device token', async () => {
    const service = createDeviceTokenService({ repo: createRepo(), tokenPepper: 'pepper' })

    const result = await service.authenticate('Device dvt_valid')
    expect(result.ok).toBe(true)
    if (result.ok) expect(result.device.id).toBe('dev_1')
  })

  it('rejects bearer access token for device socket', async () => {
    const service = createDeviceTokenService({ repo: createRepo(), tokenPepper: 'pepper' })

    await expect(service.authenticate('Bearer access_token')).resolves.toEqual({
      ok: false,
      code: ErrorCode.DEVICE_TOKEN_INVALID,
      message: '设备 token 无效'
    })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- device-token.service.test.ts
```

Expected: FAIL because `device-token.service.ts` does not exist.

- [ ] **Step 3: Implement device-token service**

Create `remote-server/src/modules/devices/device-token.service.ts`:

```ts
import { ErrorCode } from '../../shared/errors.js'
import { createHash } from '../../shared/crypto.js'

export type AuthenticatedDevice = {
  id: string
  userId: string
  name: string
  status: string
  revokedAt: Date | null
  capabilityJson: unknown
}

export type DeviceTokenRepository = {
  findActiveDeviceByTokenHash(tokenHash: string): Promise<AuthenticatedDevice | null>
  updateLastSeen(deviceId: string, lastSeenAt: Date, capabilities: unknown): Promise<void>
}

export function createDeviceTokenService(options: { repo: DeviceTokenRepository; tokenPepper: string }) {
  return {
    async authenticate(authorizationHeader: string | undefined) {
      if (!authorizationHeader?.startsWith('Device ')) {
        return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_INVALID, message: '设备 token 无效' }
      }

      const token = authorizationHeader.slice('Device '.length).trim()
      if (!token) return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_INVALID, message: '设备 token 无效' }

      const device = await options.repo.findActiveDeviceByTokenHash(createHash(token, options.tokenPepper))
      if (!device) return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_INVALID, message: '设备 token 无效' }
      if (device.status !== 'active' || device.revokedAt) {
        return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_REVOKED, message: '设备 token 已吊销' }
      }

      return { ok: true as const, device }
    }
  }
}
```

- [ ] **Step 4: Add repository methods**

Update `remote-server/src/modules/devices/devices.repository.ts`:

```ts
import { and, eq, isNull } from 'drizzle-orm'
import { devices } from '../../db/schema.js'
import type { DeviceTokenRepository } from './device-token.service.js'
import type { DevicesRepository } from './devices.service.js'

export function createDevicesRepository(db: any): DevicesRepository & DeviceTokenRepository {
  return {
    async listActiveDevices(userId) {
      return db.select().from(devices).where(and(eq(devices.userId, userId), eq(devices.status, 'active')))
    },
    async findActiveDeviceByTokenHash(tokenHash) {
      return (await db
        .select()
        .from(devices)
        .where(and(eq(devices.tokenHash, tokenHash), eq(devices.status, 'active'), isNull(devices.revokedAt)))
        .limit(1))[0] ?? null
    },
    async updateLastSeen(deviceId, lastSeenAt, capabilities) {
      await db.update(devices).set({ lastSeenAt, capabilityJson: capabilities, updatedAt: lastSeenAt }).where(eq(devices.id, deviceId))
    },
    async revokeDeviceToken(userId, deviceId, revokedAt) {
      await db.update(devices).set({ status: 'revoked', revokedAt, updatedAt: revokedAt }).where(and(eq(devices.userId, userId), eq(devices.id, deviceId)))
    }
  }
}
```

Update `remote-server/src/modules/devices/devices.service.ts` in this task so `DevicesRepository` includes the optional revoke method used by the repository:

```ts
export type DevicesRepository = {
  listActiveDevices(userId: string): Promise<Array<{
    id: string
    name: string
    lastSeenAt: Date | null
    capabilityJson: unknown
  }>>
  revokeDeviceToken?(userId: string, deviceId: string, revokedAt: Date): Promise<void>
}
```

- [ ] **Step 5: Run device-token tests and build**

Run:

```bash
cd remote-server
npm test -- device-token.service.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/modules/devices/device-token.service.ts remote-server/src/modules/devices/devices.repository.ts remote-server/tests/device-token.service.test.ts
git commit -m "feat: 新增远程设备 token 校验" -m "修改内容：新增 Device token 鉴权服务和设备 token 哈希查询、last_seen 更新仓储方法。" -m "修改原因：RemoteAgent 只能使用设备 token 连接 /ws/device，不能复用 Web access token。"
```

## Task 3: Device WebSocket Message Schemas And Socket Registry

**Files:**
- Create: `remote-server/src/ws/ws-message.schemas.ts`
- Create: `remote-server/src/modules/devices/device-socket-registry.ts`
- Test: `remote-server/tests/device-socket.test.ts`

- [ ] **Step 1: Write failing schema and registry tests**

Create `remote-server/tests/device-socket.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { deviceSocketMessageSchema } from '../src/ws/ws-message.schemas.js'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'

describe('device websocket schema and registry', () => {
  it('accepts hello and heartbeat messages', () => {
    expect(deviceSocketMessageSchema.parse({
      version: 1,
      type: 'device.hello',
      id: 'msg_1',
      data: {
        device_id: 'dev_1',
        agent_protocol_version: 1,
        rpc_protocol_version: 1,
        capabilities: { supports_webrtc: true }
      }
    }).type).toBe('device.hello')

    expect(deviceSocketMessageSchema.parse({
      version: 1,
      type: 'device.heartbeat',
      id: 'msg_2',
      data: {}
    }).type).toBe('device.heartbeat')
  })

  it('closes a registered socket when device is revoked', () => {
    const registry = createDeviceSocketRegistry()
    const close = vi.fn()

    registry.add('dev_1', { close })
    expect(registry.has('dev_1')).toBe(true)
    registry.closeDevice('dev_1', 4003, 'token_revoked')

    expect(close).toHaveBeenCalledWith(4003, 'token_revoked')
    expect(registry.has('dev_1')).toBe(false)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- device-socket.test.ts
```

Expected: FAIL because schemas and registry do not exist.

- [ ] **Step 3: Implement WebSocket message schemas**

Create `remote-server/src/ws/ws-message.schemas.ts`:

```ts
import { z } from 'zod'

const baseMessageSchema = z.object({
  version: z.literal(1),
  id: z.string().min(1).max(160)
})

export const deviceHelloMessageSchema = baseMessageSchema.extend({
  type: z.literal('device.hello'),
  data: z.object({
    device_id: z.string().min(1).max(160),
    agent_protocol_version: z.number().int().positive(),
    rpc_protocol_version: z.number().int().positive(),
    capabilities: z.record(z.unknown())
  })
})

export const deviceHeartbeatMessageSchema = baseMessageSchema.extend({
  type: z.literal('device.heartbeat'),
  data: z.object({}).default({})
})

export const deviceSocketMessageSchema = z.discriminatedUnion('type', [
  deviceHelloMessageSchema,
  deviceHeartbeatMessageSchema
])

export type DeviceSocketMessage = z.infer<typeof deviceSocketMessageSchema>
```

- [ ] **Step 4: Implement socket registry**

Create `remote-server/src/modules/devices/device-socket-registry.ts`:

```ts
export type ClosableDeviceSocket = {
  close(code: number, reason: string): void
}

export function createDeviceSocketRegistry() {
  const sockets = new Map<string, ClosableDeviceSocket>()

  return {
    add(deviceId: string, socket: ClosableDeviceSocket) {
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
    }
  }
}

export type DeviceSocketRegistry = ReturnType<typeof createDeviceSocketRegistry>
```

- [ ] **Step 5: Run schema and registry tests**

Run:

```bash
cd remote-server
npm test -- device-socket.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/ws/ws-message.schemas.ts remote-server/src/modules/devices/device-socket-registry.ts remote-server/tests/device-socket.test.ts
git commit -m "feat: 新增远程设备 WebSocket 消息模型" -m "修改内容：新增 /ws/device hello、heartbeat 消息 schema 和在线设备 socket 注册表。" -m "修改原因：服务端需要校验 RemoteAgent 消息，并在设备 token 吊销时关闭在线连接。"
```

## Task 4: /ws/device Lifecycle

**Files:**
- Create: `remote-server/src/ws/device-socket.ts`
- Modify: `remote-server/src/app.ts`
- Test: `remote-server/tests/device-socket.test.ts`

- [ ] **Step 1: Extend socket lifecycle tests**

Append to `remote-server/tests/device-socket.test.ts`:

```ts
import { handleDeviceMessage } from '../src/ws/device-socket.js'

describe('device websocket lifecycle', () => {
  it('marks device online on hello and heartbeat', async () => {
    const calls: string[] = []
    const service = {
      async markOnline(input: any) {
        calls.push(`${input.deviceId}:${input.socketId}`)
      },
      async markOffline() {}
    }
    const repo = {
      async updateLastSeen() {
        calls.push('last_seen')
      }
    }

    await handleDeviceMessage({
      raw: JSON.stringify({
        version: 1,
        type: 'device.hello',
        id: 'msg_1',
        data: {
          device_id: 'dev_1',
          agent_protocol_version: 1,
          rpc_protocol_version: 1,
          capabilities: { supports_webrtc: true }
        }
      }),
      authenticatedDevice: { id: 'dev_1', userId: 'usr_1' },
      socketId: 'sock_1',
      serverInstanceId: 'srv_1',
      presence: service,
      devices: repo
    })

    await handleDeviceMessage({
      raw: JSON.stringify({
        version: 1,
        type: 'device.heartbeat',
        id: 'msg_2',
        data: {}
      }),
      authenticatedDevice: { id: 'dev_1', userId: 'usr_1' },
      socketId: 'sock_1',
      serverInstanceId: 'srv_1',
      presence: service,
      devices: repo
    })

    expect(calls).toContain('dev_1:sock_1')
    expect(calls).toContain('last_seen')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- device-socket.test.ts
```

Expected: FAIL because `device-socket.ts` does not exist.

- [ ] **Step 3: Implement message handler**

Create `remote-server/src/ws/device-socket.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import websocket from '@fastify/websocket'
import { loadConfigFromEnv } from '../config.js'
import { createDb } from '../db/client.js'
import { createRedis } from '../redis/client.js'
import { createDevicesRepository } from '../modules/devices/devices.repository.js'
import { createDeviceTokenService, type AuthenticatedDevice } from '../modules/devices/device-token.service.js'
import { createPresenceService } from '../modules/devices/presence.service.js'
import { createDeviceSocketRegistry, type DeviceSocketRegistry } from '../modules/devices/device-socket-registry.js'
import { deviceSocketMessageSchema } from './ws-message.schemas.js'

export type DeviceMessageDeps = {
  raw: string
  authenticatedDevice: Pick<AuthenticatedDevice, 'id' | 'userId'>
  socketId: string
  serverInstanceId: string
  presence: {
    markOnline(input: {
      userId: string
      deviceId: string
      socketId: string
      serverInstanceId: string
      lastSeenAt: string
      capabilities: unknown
    }): Promise<void>
  }
  devices: {
    updateLastSeen(deviceId: string, lastSeenAt: Date, capabilities: unknown): Promise<void>
  }
}

export async function handleDeviceMessage(deps: DeviceMessageDeps) {
  const message = deviceSocketMessageSchema.parse(JSON.parse(deps.raw))
  const now = new Date()

  if (message.type === 'device.hello') {
    if (message.data.device_id !== deps.authenticatedDevice.id) {
      throw new Error('device_id_mismatch')
    }
    await deps.presence.markOnline({
      userId: deps.authenticatedDevice.userId,
      deviceId: deps.authenticatedDevice.id,
      socketId: deps.socketId,
      serverInstanceId: deps.serverInstanceId,
      lastSeenAt: now.toISOString(),
      capabilities: message.data.capabilities
    })
    await deps.devices.updateLastSeen(deps.authenticatedDevice.id, now, message.data.capabilities)
    return
  }

  await deps.presence.markOnline({
    userId: deps.authenticatedDevice.userId,
    deviceId: deps.authenticatedDevice.id,
    socketId: deps.socketId,
    serverInstanceId: deps.serverInstanceId,
    lastSeenAt: now.toISOString(),
    capabilities: {}
  })
  await deps.devices.updateLastSeen(deps.authenticatedDevice.id, now, {})
}

export async function registerDeviceSocket(app: FastifyInstance, registry: DeviceSocketRegistry = createDeviceSocketRegistry()) {
  await app.register(websocket)

  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const redis = createRedis(config.redisUrl)
  const devicesRepo = createDevicesRepository(db)
  const deviceTokenService = createDeviceTokenService({
    repo: devicesRepo,
    tokenPepper: config.tokenPepper
  })
  const presence = createPresenceService({
    redis,
    ttlSeconds: config.devicePresenceTtlSeconds
  })
  const serverInstanceId = `srv_${process.pid}`

  app.get('/ws/device', { websocket: true }, async (socket, request) => {
    const auth = await deviceTokenService.authenticate(request.headers.authorization)
    if (!auth.ok) {
      socket.close(4001, JSON.stringify({ code: auth.code, message: auth.message }))
      return
    }

    const socketId = `sock_${Date.now()}_${Math.random().toString(36).slice(2)}`
    registry.add(auth.device.id, socket)

    socket.on('message', async (raw) => {
      try {
        await handleDeviceMessage({
          raw: raw.toString(),
          authenticatedDevice: auth.device,
          socketId,
          serverInstanceId,
          presence,
          devices: devicesRepo
        })
      } catch {
        socket.close(4002, 'invalid_device_message')
      }
    })

    socket.on('close', async () => {
      registry.remove(auth.device.id)
      await presence.markOffline(auth.device.id)
    })
  })
}
```

- [ ] **Step 4: Register device socket in app**

Update `remote-server/src/app.ts`:

```ts
import { registerDeviceSocket } from './ws/device-socket.js'
import { createDeviceSocketRegistry, type DeviceSocketRegistry } from './modules/devices/device-socket-registry.js'
```

Inside `buildApp()` create the registry once:

```ts
  const deviceSocketRegistry = createDeviceSocketRegistry()
```

Extend the existing `AppDeps` from the auth plan:

```ts
export type AppDeps = {
  registerAuthRoutes?: (app: FastifyInstance) => Promise<void>
  registerDesktopLoginRoutes?: (app: FastifyInstance) => Promise<void>
  registerDevicesRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerDeviceSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
}
```

After route registration, pass the same registry to both devices routes and `/ws/device`:

```ts
  void (deps.registerDevicesRoutes ?? registerDevicesRoutes)(app, { registry: deviceSocketRegistry })
  void (deps.registerDeviceSocket ?? registerDeviceSocket)(app, deviceSocketRegistry)
```

- [ ] **Step 5: Run lifecycle tests and build**

Run:

```bash
cd remote-server
npm test -- device-socket.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/ws/device-socket.ts remote-server/src/app.ts remote-server/tests/device-socket.test.ts
git commit -m "feat: 新增远程设备 WebSocket 连接" -m "修改内容：新增 /ws/device 鉴权、hello、heartbeat、presence 写入和断开清理逻辑。" -m "修改原因：本机 RemoteAgent 需要主动连接远程服务端并上报在线可用状态。"
```

## Task 5: Device List Presence Merge And Revoke Token API

**Files:**
- Modify: `remote-server/src/modules/devices/devices.service.ts`
- Modify: `remote-server/src/modules/devices/devices.routes.ts`
- Modify: `remote-server/src/modules/devices/devices.schemas.ts`
- Test: `remote-server/tests/devices.presence.routes.test.ts`

- [ ] **Step 1: Write failing device presence route tests**

Create `remote-server/tests/devices.presence.routes.test.ts`:

```ts
import { describe, expect, it, vi } from 'vitest'
import { createDevicesService } from '../src/modules/devices/devices.service.js'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'

describe('device list presence merge and revoke token', () => {
  it('marks devices online when Redis presence exists', async () => {
    const service = createDevicesService({
      repo: {
        async listActiveDevices() {
          return [{
            id: 'dev_1',
            name: 'NiuMa MacBook',
            lastSeenAt: new Date('2026-06-28T00:00:00.000Z'),
            capabilityJson: { supports_webrtc: true }
          }]
        }
      },
      presence: {
        async getPresence(deviceId: string) {
          return deviceId === 'dev_1'
            ? {
                user_id: 'usr_1',
                device_id: 'dev_1',
                socket_id: 'sock_1',
                server_instance_id: 'srv_1',
                last_seen_at: '2026-06-28T00:01:00.000Z',
                capabilities: { supports_webrtc: true }
              }
            : null
        }
      }
    })

    await expect(service.list('usr_1')).resolves.toEqual({
      list: [{
        id: 'dev_1',
        name: 'NiuMa MacBook',
        online: true,
        last_seen_at: '2026-06-28T00:01:00.000Z',
        capabilities: { supports_webrtc: true }
      }]
    })
  })

  it('closes online socket when token is revoked', async () => {
    const registry = createDeviceSocketRegistry()
    const close = vi.fn()
    registry.add('dev_1', { close })

    registry.closeDevice('dev_1', 4003, 'token_revoked')

    expect(close).toHaveBeenCalledWith(4003, 'token_revoked')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- devices.presence.routes.test.ts
```

Expected: FAIL because `createDevicesService` does not accept `presence`.

- [ ] **Step 3: Update device service**

Update `remote-server/src/modules/devices/devices.service.ts`:

```ts
import type { PresenceRecord } from './presence.service.js'

export type DeviceListItem = {
  id: string
  name: string
  online: boolean
  last_seen_at: string | null
  capabilities: unknown
}

export type DevicesRepository = {
  listActiveDevices(userId: string): Promise<Array<{
    id: string
    name: string
    lastSeenAt: Date | null
    capabilityJson: unknown
  }>>
  revokeDeviceToken?(userId: string, deviceId: string, revokedAt: Date): Promise<void>
}

export type DevicePresenceReader = {
  getPresence(deviceId: string): Promise<PresenceRecord | null>
  markOffline?(deviceId: string): Promise<void>
}

export function createDevicesService(options: { repo: DevicesRepository; presence?: DevicePresenceReader }) {
  return {
    async list(userId: string): Promise<{ list: DeviceListItem[] }> {
      const devices = await options.repo.listActiveDevices(userId)
      const list = await Promise.all(devices.map(async (device) => {
        const presence = options.presence ? await options.presence.getPresence(device.id) : null
        return {
          id: device.id,
          name: device.name,
          online: Boolean(presence),
          last_seen_at: presence?.last_seen_at ?? device.lastSeenAt?.toISOString() ?? null,
          capabilities: presence?.capabilities ?? device.capabilityJson
        }
      }))
      return { list }
    },

    async revokeToken(input: { userId: string; deviceId: string; now: Date }) {
      if (!options.repo.revokeDeviceToken) throw new Error('revokeDeviceToken not implemented')
      await options.repo.revokeDeviceToken(input.userId, input.deviceId, input.now)
      if (options.presence?.markOffline) await options.presence.markOffline(input.deviceId)
      return {}
    }
  }
}
```

- [ ] **Step 4: Update device schemas**

Update `remote-server/src/modules/devices/devices.schemas.ts`:

```ts
export const deviceRevokeTokenSchema = z.object({
  device_id: deviceIdSchema
})
```

- [ ] **Step 5: Update devices route**

Update `remote-server/src/modules/devices/devices.routes.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { createRedis } from '../../redis/client.js'
import { apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { requireAuth } from '../auth/auth.middleware.js'
import { createDevicesRepository } from './devices.repository.js'
import { createDevicesService } from './devices.service.js'
import { createPresenceService } from './presence.service.js'
import { deviceRevokeTokenSchema } from './devices.schemas.js'
import type { DeviceSocketRegistry } from './device-socket-registry.js'
```

Define the route registration signature and build service with presence:

```ts
export async function registerDevicesRoutes(app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) {
  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const redis = createRedis(config.redisUrl)
  const presence = createPresenceService({
    redis,
    ttlSeconds: config.devicePresenceTtlSeconds
  })
  const service = createDevicesService({
    repo: createDevicesRepository(db),
    presence
  })
}
```

Add route:

```ts
  app.post('/api/v1/devices/revoke-token', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response

    const parsed = parseBody(deviceRevokeTokenSchema, request.body)
    if (!parsed.ok) return parsed.response

    await service.revokeToken({
      userId: auth.auth.userId,
      deviceId: parsed.data.device_id,
      now: new Date()
    })
    deps.registry.closeDevice(parsed.data.device_id, 4003, 'token_revoked')
    return apiSuccess({})
  })
```

The same `DeviceSocketRegistry` instance is now shared between `/ws/device` and the devices route through `buildApp`.

- [ ] **Step 6: Run route tests and build**

Run:

```bash
cd remote-server
npm test -- devices.presence.routes.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/devices/devices.service.ts remote-server/src/modules/devices/devices.routes.ts remote-server/src/modules/devices/devices.schemas.ts remote-server/tests/devices.presence.routes.test.ts
git commit -m "feat: 合并远程设备在线状态" -m "修改内容：设备列表合并 Redis presence 在线状态，并新增设备 token 吊销接口关闭在线 socket。" -m "修改原因：外部控制台需要判断设备是否在线可用，用户也需要能吊销设备凭据。"
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

- [ ] **Step 2: Verify WebSocket routes are not using API envelope**

Run:

```bash
rg -n "apiSuccess|apiFailure" remote-server/src/ws
```

Expected: no output.

- [ ] **Step 3: Verify device token cannot call user API**

Run:

```bash
rg -n "startsWith\\('Device '|Authorization: Device|Bearer" remote-server/src/modules remote-server/src/ws
```

Expected: `Device ` handling appears only in `device-token.service.ts` or `/ws/device` code; ordinary HTTP user routes still use Bearer access token middleware.

- [ ] **Step 4: Verify no dynamic path params in added HTTP routes**

Run:

```bash
rg -n "app\\.(get|post|put|delete)\\('/api/[^']*:" remote-server/src/modules remote-server/src/ws
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
Remote server device presence slice complete:
- Device token validation for /ws/device
- device.hello and device.heartbeat message validation
- Redis presence writes and deletes
- Device list online status from Redis
- Device token revoke closes online socket in this server instance

Verification:
- cd remote-server && npm run check
- rg WebSocket envelope scan
- rg Device token scope scan
- rg dynamic API path scan
```

Do not mark remote server complete after this milestone. Connection creation, connection token, `/ws/client`, WebRTC signaling, `/ws/relay`, and cross-instance token-revocation fanout remain separate milestones.

## Self-Review

Spec coverage in this plan:

- Device token can connect `/ws/device`: covered by Tasks 2 and 4.
- Access token cannot replace device token: covered by Task 2 tests and Task 6 scan.
- `/ws/device` writes `presence:device:{device_id}`: covered by Tasks 1 and 4.
- RemoteAgent hello and heartbeat messages: covered by Tasks 3 and 4.
- Device list merges PostgreSQL records with Redis online state: covered by Task 5.
- Revoked token closes online connection in current server instance: covered by Tasks 3 and 5.
- WebSocket messages do not use HTTP envelope: covered by Task 6.

Known follow-up plans:

- Implement device rename and unbind route behavior.
- Implement connection creation, connection token, and ICE config.
- Implement `/ws/client` signaling.
- Implement `/ws/relay` ciphertext forwarding.
- Add Redis pub/sub fanout so token revocation closes sockets across multiple server instances.
