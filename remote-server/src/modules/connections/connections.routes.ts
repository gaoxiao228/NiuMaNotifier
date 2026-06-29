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
export { createConnectionInviteMessage } from './connection-invite.js'
import { createConnectionsRepository } from './connections.repository.js'
import { connectionCreateSchema } from './connections.schemas.js'
import { createConnectionsService } from './connections.service.js'

export async function registerConnectionsRoutes(
  app: FastifyInstance,
  _deps: { registry: DeviceSocketRegistry }
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

    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.get('/api/v1/connections/ice-config', async () => {
    return apiSuccess(service.iceConfig(config.turn))
  })
}
