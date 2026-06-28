import Fastify, { type FastifyInstance } from 'fastify'
import {
  createDeviceSocketRegistry,
  type DeviceSocketRegistry
} from './modules/devices/device-socket-registry.js'
import { registerConnectionsRoutes } from './modules/connections/connections.routes.js'
import { registerHealthRoutes } from './modules/health/health.routes.js'
import { ErrorCode } from './shared/errors.js'
import { apiFailure } from './shared/response.js'
import { registerDeviceSocket } from './ws/device-socket.js'

export type AppDeps = {
  registerAuthRoutes?: (app: FastifyInstance) => Promise<void>
  registerDesktopLoginRoutes?: (app: FastifyInstance) => Promise<void>
  registerDevicesRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerConnectionsRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerDeviceSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
}

export function buildApp(deps: AppDeps = {}) {
  const app = Fastify({ logger: false })
  const deviceSocketRegistry = createDeviceSocketRegistry()

  void registerHealthRoutes(app)
  if (deps.registerAuthRoutes) void deps.registerAuthRoutes(app)
  if (deps.registerDesktopLoginRoutes) void deps.registerDesktopLoginRoutes(app)
  if (deps.registerDevicesRoutes) void deps.registerDevicesRoutes(app, { registry: deviceSocketRegistry })
  if (deps.registerConnectionsRoutes) void deps.registerConnectionsRoutes(app, { registry: deviceSocketRegistry })
  if (deps.registerDeviceSocket) void deps.registerDeviceSocket(app, deviceSocketRegistry)

  app.setNotFoundHandler(async (_request, reply) => {
    return reply.status(404).send(apiFailure(ErrorCode.ROUTE_NOT_FOUND, '接口不存在'))
  })

  app.setErrorHandler(async (_error, _request, reply) => {
    return reply.status(500).send(apiFailure(ErrorCode.SYSTEM_ERROR, '系统异常'))
  })

  return app
}
