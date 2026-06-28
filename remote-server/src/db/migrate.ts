import { migrate } from 'drizzle-orm/node-postgres/migrator'
import { loadConfigFromEnv } from '../config.js'
import { createDb } from './client.js'

const config = loadConfigFromEnv()
const { db, pool } = createDb(config.databaseUrl)

try {
  await migrate(db, { migrationsFolder: './migrations' })
} finally {
  await pool.end()
}
