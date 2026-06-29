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
import { createConnectionInviteMessage } from '../modules/connections/connection-invite.js'
import type { DeviceSocketRegistry } from '../modules/devices/device-socket-registry.js'
import { createRedis } from '../redis/client.js'
import { ensureWebsocketRegistered } from './websocket-plugin.js'

function messageId(prefix: string) {
  return `msg_${prefix}_${Date.now().toString(36)}`
}

export type BoundClientConnection = {
  connectionId: string
  userId: string
  deviceId: string
  clientId: string
  connectionToken: string
  transportPreference: 'webrtc_first' | 'relay_first' | 'relay_only'
  expiresAt: string
}

export async function bindClientConnection(input: {
  query: unknown
  tokenPepper: string
  state: { get(connectionId: string): Promise<ConnectionState | null> }
}): Promise<{ ok: true; connection: BoundClientConnection } | { ok: false; code: number; message: string }> {
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

  return {
    ok: true,
    connection: {
      connectionId: state.connection_id,
      userId: state.user_id,
      deviceId: state.device_id,
      clientId: state.client_id,
      connectionToken: parsed.data.connection_token,
      transportPreference: state.transport_preference ?? 'webrtc_first',
      expiresAt: state.expires_at
    }
  }
}

export function inviteDeviceForBoundClient(input: {
  connection: BoundClientConnection
  registry: { sendToDevice(deviceId: string, message: object): boolean }
}) {
  return input.registry.sendToDevice(input.connection.deviceId, createConnectionInviteMessage({
    connectionId: input.connection.connectionId,
    connectionToken: input.connection.connectionToken,
    clientId: input.connection.clientId,
    transportPreference: input.connection.transportPreference,
    expiresAt: input.connection.expiresAt
  }))
}

export function createClientCancelMessage(connectionId: string) {
  return {
    version: 1,
    type: 'signal.cancel',
    id: messageId(connectionId),
    data: {
      connection_id: connectionId,
      reason: 'client_closed'
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
    data: {
      ...message.data,
      connection_id: input.connection.connectionId,
      client_id: input.connection.clientId
    }
  })

  return sent ? { ok: true as const } : { ok: false as const, code: 210404, message: '设备离线' }
}

export async function registerClientSocket(app: FastifyInstance, registry: DeviceSocketRegistry) {
  await ensureWebsocketRegistered(app)

  const config = loadConfigFromEnv()
  const redis = createRedis(config.redisUrl)
  const state = createConnectionStateService({
    redis,
    ttlSeconds: config.connectionTokenTtlSeconds
  })

  app.get('/ws/client', { websocket: true }, async (socket, request) => {
    const bound = await bindClientConnection({
      query: request.query,
      tokenPepper: config.tokenPepper,
      state
    })
    if (!bound.ok) {
      socket.close(4003, JSON.stringify({ code: bound.code, message: bound.message }))
      return
    }

    registry.bindClient(bound.connection.connectionId, socket)
    if (!inviteDeviceForBoundClient({ connection: bound.connection, registry })) {
      socket.close(4004, JSON.stringify({ code: 210404, message: '设备离线' }))
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

    socket.on('close', () => {
      registry.unbindClient(bound.connection.connectionId, socket)
      registry.sendToDevice(bound.connection.deviceId, createClientCancelMessage(bound.connection.connectionId))
    })
  })
}
