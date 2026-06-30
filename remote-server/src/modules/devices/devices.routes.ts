import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { createRedis } from '../../redis/client.js'
import { apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { requireAuth } from '../auth/auth.middleware.js'
import type { DeviceSocketRegistry } from './device-socket-registry.js'
import { createDevicesRepository } from './devices.repository.js'
import { deviceRevokeTokenSchema } from './devices.schemas.js'
import { createDevicesService } from './devices.service.js'
import { createPresenceService } from './presence.service.js'

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

  app.get('/api/v1/devices/list', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response

    return apiSuccess(await service.list(auth.auth.userId))
  })

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
}
