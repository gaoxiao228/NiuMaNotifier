import { buildApp } from './app.js'
import { loadConfigFromEnv } from './config.js'

const config = loadConfigFromEnv()
const app = buildApp()

await app.listen({ host: config.bind, port: config.port })
