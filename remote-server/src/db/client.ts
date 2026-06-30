import { drizzle } from 'drizzle-orm/node-postgres'
import pg from 'pg'
import * as schema from './schema.js'

export function createDb(databaseUrl: string) {
  const pool = new pg.Pool({ connectionString: databaseUrl })

  return {
    pool,
    db: drizzle(pool, { schema })
  }
}
