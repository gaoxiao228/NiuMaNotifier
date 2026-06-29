import Fastify, { type FastifyInstance } from 'fastify'
import {
  createDeviceSocketRegistry,
  type DeviceSocketRegistry
} from './modules/devices/device-socket-registry.js'
import { registerConnectionsRoutes } from './modules/connections/connections.routes.js'
import { registerHealthRoutes } from './modules/health/health.routes.js'
import { registerWebConsoleRoutes } from './modules/webConsole/webConsole.routes.js'
import { ErrorCode } from './shared/errors.js'
import { apiFailure } from './shared/response.js'
import { registerClientSocket } from './ws/client-socket.js'
import { registerDeviceSocket } from './ws/device-socket.js'
import { registerRelaySocket } from './ws/relay-socket.js'
import { ensureWebsocketRegistered } from './ws/websocket-plugin.js'

export type AppDeps = {
  registerAuthRoutes?: (app: FastifyInstance) => Promise<void>
  registerDesktopLoginRoutes?: (app: FastifyInstance) => Promise<void>
  registerDevicesRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerConnectionsRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerDeviceSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
  registerClientSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
  registerRelaySocket?: (app: FastifyInstance) => Promise<void>
  registerWebConsoleRoutes?: (app: FastifyInstance) => Promise<void>
}

export function buildApp(deps: AppDeps = {}) {
  const app = Fastify({ logger: false })
  const deviceSocketRegistry = createDeviceSocketRegistry()

  void app.register(async (instance) => {
    if (deps.registerDeviceSocket || deps.registerClientSocket || deps.registerRelaySocket) {
      await ensureWebsocketRegistered(instance)
    }

    await registerHealthRoutes(instance)
    if (deps.registerAuthRoutes) await deps.registerAuthRoutes(instance)
    if (deps.registerDesktopLoginRoutes) await deps.registerDesktopLoginRoutes(instance)
    if (deps.registerDevicesRoutes) await deps.registerDevicesRoutes(instance, { registry: deviceSocketRegistry })
    if (deps.registerConnectionsRoutes) await deps.registerConnectionsRoutes(instance, { registry: deviceSocketRegistry })
    if (deps.registerDeviceSocket) await deps.registerDeviceSocket(instance, deviceSocketRegistry)
    if (deps.registerClientSocket) await deps.registerClientSocket(instance, deviceSocketRegistry)
    if (deps.registerRelaySocket) await deps.registerRelaySocket(instance)
    await (deps.registerWebConsoleRoutes ?? registerWebConsoleRoutes)(instance)
  })

  app.setNotFoundHandler(async (_request, reply) => {
    return reply.status(404).send(apiFailure(ErrorCode.ROUTE_NOT_FOUND, '接口不存在'))
  })

  app.setErrorHandler(async (error, _request, reply) => {
    const message = error instanceof Error ? error.message : String(error)
    console.error(`NiuMaNotifier request error: ${message}`)
    return reply.status(500).send(apiFailure(ErrorCode.SYSTEM_ERROR, '系统异常'))
  })

  return app
}
