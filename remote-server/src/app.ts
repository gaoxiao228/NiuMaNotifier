import Fastify from 'fastify'
import { registerHealthRoutes } from './modules/health/health.routes.js'
import { ErrorCode } from './shared/errors.js'
import { apiFailure } from './shared/response.js'

export function buildApp() {
  const app = Fastify({ logger: false })

  void registerHealthRoutes(app)

  app.setNotFoundHandler(async (_request, reply) => {
    return reply.status(404).send(apiFailure(ErrorCode.ROUTE_NOT_FOUND, '接口不存在'))
  })

  app.setErrorHandler(async (_error, _request, reply) => {
    return reply.status(500).send(apiFailure(ErrorCode.SYSTEM_ERROR, '系统异常'))
  })

  return app
}
