import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { apiSuccess } from '../../shared/response.js'
import { requireAuth } from '../auth/auth.middleware.js'
import { createDevicesRepository } from './devices.repository.js'
import { createDevicesService } from './devices.service.js'

export async function registerDevicesRoutes(app: FastifyInstance) {
  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const service = createDevicesService({
    repo: createDevicesRepository(db)
  })

  app.get('/api/v1/devices/list', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response

    return apiSuccess(await service.list(auth.auth.userId))
  })
}
