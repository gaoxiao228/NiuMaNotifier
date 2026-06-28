import Fastify, { type FastifyInstance } from 'fastify'
import { registerHealthRoutes } from './modules/health/health.routes.js'
import { ErrorCode } from './shared/errors.js'
import { apiFailure } from './shared/response.js'

export type AppDeps = {
  registerAuthRoutes?: (app: FastifyInstance) => Promise<void>
  registerDesktopLoginRoutes?: (app: FastifyInstance) => Promise<void>
  registerDevicesRoutes?: (app: FastifyInstance) => Promise<void>
}

export function buildApp(deps: AppDeps = {}) {
  const app = Fastify({ logger: false })

  void registerHealthRoutes(app)
  if (deps.registerAuthRoutes) void deps.registerAuthRoutes(app)
  if (deps.registerDesktopLoginRoutes) void deps.registerDesktopLoginRoutes(app)
  if (deps.registerDevicesRoutes) void deps.registerDevicesRoutes(app)

  app.setNotFoundHandler(async (_request, reply) => {
    return reply.status(404).send(apiFailure(ErrorCode.ROUTE_NOT_FOUND, '接口不存在'))
  })

  app.setErrorHandler(async (_error, _request, reply) => {
    return reply.status(500).send(apiFailure(ErrorCode.SYSTEM_ERROR, '系统异常'))
  })

  return app
}
