import { buildApp } from './app.js'
import { loadConfigFromEnv } from './config.js'
import { registerAuthRoutes } from './modules/auth/auth.routes.js'
import { registerDesktopLoginRoutes } from './modules/desktopLogin/desktopLogin.routes.js'
import { registerDevicesRoutes } from './modules/devices/devices.routes.js'

const config = loadConfigFromEnv()
const app = buildApp({ registerAuthRoutes, registerDesktopLoginRoutes, registerDevicesRoutes })

await app.listen({ host: config.bind, port: config.port })
