import type { FastifyInstance } from 'fastify'
import { apiSuccess } from '../../shared/response.js'

export async function registerHealthRoutes(app: FastifyInstance) {
  app.get('/api/v1/health', async () =>
    apiSuccess({
      service: 'niuma-remote-server',
      status: 'ok'
    })
  )
}
