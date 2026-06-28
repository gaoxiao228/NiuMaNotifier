import { buildApp } from './app.js'
import { loadConfigFromEnv } from './config.js'
import { registerAuthRoutes } from './modules/auth/auth.routes.js'
import { registerConnectionsRoutes } from './modules/connections/connections.routes.js'
import { registerDesktopLoginRoutes } from './modules/desktopLogin/desktopLogin.routes.js'
import { registerDevicesRoutes } from './modules/devices/devices.routes.js'
import { registerClientSocket } from './ws/client-socket.js'
import { registerDeviceSocket } from './ws/device-socket.js'
import { registerRelaySocket } from './ws/relay-socket.js'

const config = loadConfigFromEnv()
const app = buildApp({
  registerAuthRoutes,
  registerDesktopLoginRoutes,
  registerDevicesRoutes,
  registerConnectionsRoutes,
  registerDeviceSocket,
  registerClientSocket,
  registerRelaySocket
})

await app.listen({ host: config.bind, port: config.port })
