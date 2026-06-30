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

const defaultCorsOrigins = ['http://127.0.0.1:27883']
const corsMethods = 'GET,POST,OPTIONS'
const corsHeaders = 'Authorization,Content-Type,Accept'

export type AppDeps = {
  registerAuthRoutes?: (app: FastifyInstance) => Promise<void>
  registerDesktopLoginRoutes?: (app: FastifyInstance) => Promise<void>
  registerDevicesRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerConnectionsRoutes?: (app: FastifyInstance, deps: { registry: DeviceSocketRegistry }) => Promise<void>
  registerDeviceSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
  registerClientSocket?: (app: FastifyInstance, registry: DeviceSocketRegistry) => Promise<void>
  registerRelaySocket?: (app: FastifyInstance) => Promise<void>
  registerWebConsoleRoutes?: (app: FastifyInstance) => Promise<void>
  corsOrigins?: string[]
}

function getAllowedCorsOrigin(origin: string | undefined, allowedOrigins: string[]) {
  if (!origin) return null
  if (allowedOrigins.includes('*') || allowedOrigins.includes(origin)) return origin
  return null
}

export function buildApp(deps: AppDeps = {}) {
  const app = Fastify({ logger: false })
  const deviceSocketRegistry = createDeviceSocketRegistry()
  const corsOrigins = deps.corsOrigins ?? defaultCorsOrigins

  app.addHook('onRequest', async (request, reply) => {
    const allowedOrigin = getAllowedCorsOrigin(request.headers.origin, corsOrigins)
    if (!allowedOrigin) return

    // 外部客户端使用 Bearer token，不需要 Cookie；这里只开放浏览器跨源 API/RPC 调用需要的最小头。
    reply.header('Access-Control-Allow-Origin', allowedOrigin)
    reply.header('Vary', 'Origin')
    reply.header('Access-Control-Allow-Methods', corsMethods)
    reply.header('Access-Control-Allow-Headers', corsHeaders)
  })

  app.options('/*', async (_request, reply) => {
    return reply.status(204).send()
  })

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
