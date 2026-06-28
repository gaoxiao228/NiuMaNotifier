import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { createRedis } from '../../redis/client.js'
import { apiFailure, apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { requireAuth } from '../auth/auth.middleware.js'
import type { DeviceSocketRegistry } from '../devices/device-socket-registry.js'
import { createPresenceService } from '../devices/presence.service.js'
import { createConnectionStateService } from './connection-state.service.js'
import { createConnectionsRepository } from './connections.repository.js'
import { connectionCreateSchema } from './connections.schemas.js'
import { createConnectionsService } from './connections.service.js'

function mapInviteTransportPreference(input: 'webrtc_first' | 'relay_first' | 'relay_only') {
  // 本机 Task 5 已支持 relay accept；完整 relay ping/pong 由后续 runtime 接入。
  if (input === 'webrtc_first') return 'auto'
  return 'relay'
}

export function createConnectionInviteMessage(input: {
  connectionId: string
  clientId: string
  transportPreference: 'webrtc_first' | 'relay_first' | 'relay_only'
  expiresAt: string
}) {
  return {
    version: 1,
    type: 'connection.invite',
    id: `msg_${input.connectionId}`,
    data: {
      connection_id: input.connectionId,
      client_id: input.clientId,
      transport_preference: mapInviteTransportPreference(input.transportPreference),
      expires_at: input.expiresAt
    }
  }
}

export async function registerConnectionsRoutes(
  app: FastifyInstance,
  deps: { registry: DeviceSocketRegistry }
) {
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
    const transportPreference = parsed.data.transport_preference ?? 'webrtc_first'

    const result = await service.create({
      userId: auth.auth.userId,
      deviceId: parsed.data.device_id,
      clientId: parsed.data.client_id,
      transportPreference
    })

    if (result.ok) {
      deps.registry.sendToDevice(parsed.data.device_id, createConnectionInviteMessage({
        connectionId: result.data.connection_id,
        clientId: parsed.data.client_id,
        transportPreference,
        expiresAt: result.data.expires_at
      }))
    }

    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.get('/api/v1/connections/ice-config', async () => {
    return apiSuccess(service.iceConfig(config.turn))
  })
}
