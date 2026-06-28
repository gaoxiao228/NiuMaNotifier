import websocket from '@fastify/websocket'
import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../config.js'
import {
  createConnectionStateService,
  type ConnectionState
} from '../modules/connections/connection-state.service.js'
import { createConnectionTokenService } from '../modules/connections/connection-token.service.js'
import {
  clientSignalMessageSchema,
  connectionClientBindSchema
} from '../modules/connections/connections.schemas.js'
import { requireAuth, type AuthContext } from '../modules/auth/auth.middleware.js'
import type { DeviceSocketRegistry } from '../modules/devices/device-socket-registry.js'
import { createRedis } from '../redis/client.js'

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
  if (new Date(state.expires_at).getTime() <= Date.now()) {
    return { ok: false, code: 220402, message: '连接已过期' }
  }

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

    socket.on('message', async (raw: { toString(): string }) => {
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
