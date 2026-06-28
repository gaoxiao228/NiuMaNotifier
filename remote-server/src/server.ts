import { buildApp } from './app.js'
import { loadConfigFromEnv } from './config.js'
import { registerAuthRoutes } from './modules/auth/auth.routes.js'

const config = loadConfigFromEnv()
const app = buildApp({ registerAuthRoutes })

await app.listen({ host: config.bind, port: config.port })
