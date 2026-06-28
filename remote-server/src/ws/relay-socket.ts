import type { FastifyInstance, FastifyRequest } from 'fastify'
import { loadConfigFromEnv } from '../config.js'
import { createDb } from '../db/client.js'
import { createConnectionStateService, type ConnectionState } from '../modules/connections/connection-state.service.js'
import { createConnectionTokenService } from '../modules/connections/connection-token.service.js'
import { createDeviceTokenService } from '../modules/devices/device-token.service.js'
import { createDevicesRepository } from '../modules/devices/devices.repository.js'
import { createRelayRouteService } from '../modules/relay/relay-route.service.js'
import { createRelaySocketRegistry, type RelaySocketRegistry } from '../modules/relay/relay-socket-registry.js'
import { relayBindSchema, relayFrameSchema, type RelaySide } from '../modules/relay/relay.schemas.js'
import { createRedis } from '../redis/client.js'
import { createPublicId } from '../shared/id.js'
import { ensureWebsocketRegistered } from './websocket-plugin.js'

export type RelayActor =
  | { kind: 'client' }
  | { kind: 'device'; deviceId: string }

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
  if (new Date(state.expires_at).getTime() <= Date.now()) {
    return { ok: false, code: 220402, message: '连接已过期' }
  }

  const tokenService = createConnectionTokenService({ tokenPepper: input.tokenPepper })
  if (!tokenService.verify(parsed.data.connection_token, state.token_hash)) {
    return { ok: false, code: 220403, message: '连接权限不足' }
  }

  if (parsed.data.side === 'client' && input.actor.kind !== 'client') {
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

async function resolveClientActor(_request: FastifyRequest) {
  // 浏览器 WebSocket 无法携带自定义 Authorization header；client side 只通过短期连接 token 绑定。
  return { ok: true as const, actor: { kind: 'client' as const } }
}

async function resolveDeviceActor(
  request: FastifyRequest,
  deviceTokenService: ReturnType<typeof createDeviceTokenService>
) {
  const auth = await deviceTokenService.authenticate(request.headers.authorization)
  return auth.ok
    ? { ok: true as const, actor: { kind: 'device' as const, deviceId: auth.device.id } }
    : { ok: false as const, code: auth.code, message: auth.message }
}

export async function registerRelaySocket(
  app: FastifyInstance,
  registry: RelaySocketRegistry = createRelaySocketRegistry()
) {
  await ensureWebsocketRegistered(app)

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
      ? await resolveClientActor(request)
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

    const socketId = createPublicId('relay')
    registry.add({
      connectionId: bound.binding.connectionId,
      side: bound.binding.side,
      socketId,
      socket
    })
    await route.setRoute({
      connectionId: bound.binding.connectionId,
      clientSocketId: bound.binding.side === 'client'
        ? socketId
        : registry.getSocketId(bound.binding.connectionId, 'client'),
      deviceSocketId: bound.binding.side === 'device'
        ? socketId
        : registry.getSocketId(bound.binding.connectionId, 'device'),
      serverInstanceId,
      startedAt: new Date().toISOString()
    })

    socket.on('message', async (raw: { toString(): string }) => {
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
