import { eq } from 'drizzle-orm'
import { pathToFileURL } from 'node:url'
import { loadConfigFromEnv } from '../config.js'
import { users } from './schema.js'
import { createDb } from './client.js'
import { hashPassword } from '../modules/auth/password.service.js'

export type BootstrapAdminRepository = {
  findAdmin(): Promise<unknown | null>
  findUserByEmail(email: string): Promise<{ role: string } | null>
  createUser(input: {
    email: string
    passwordHash: string
    passwordAlgo: 'argon2id'
    role: 'admin'
    status: 'active'
    createdAt: Date
    updatedAt: Date
    passwordUpdatedAt: Date
  }): Promise<unknown>
}

export function createBootstrapAdminService(options: { repo: BootstrapAdminRepository }) {
  return {
    async bootstrap(input: { email: string; password: string; now: Date }) {
      const existingAdmin = await options.repo.findAdmin()
      if (existingAdmin) return { created: false, skipped: true }

      const email = input.email.toLowerCase()
      const existingUser = await options.repo.findUserByEmail(email)
      if (existingUser) {
        // 避免把已存在的普通账号静默提升为管理员；管理员身份必须显式创建。
        throw new Error('BOOTSTRAP_ADMIN_EMAIL 已属于普通用户，不能自动提升为管理员')
      }

      const password = await hashPassword(input.password)
      await options.repo.createUser({
        email,
        passwordHash: password.hash,
        passwordAlgo: password.algo,
        role: 'admin',
        status: 'active',
        createdAt: input.now,
        updatedAt: input.now,
        passwordUpdatedAt: input.now
      })

      return { created: true, skipped: false }
    }
  }
}

function createBootstrapAdminRepository(db: any): BootstrapAdminRepository {
  return {
    async findAdmin() {
      return (await db.select().from(users).where(eq(users.role, 'admin')).limit(1))[0] ?? null
    },
    async findUserByEmail(email) {
      return (await db.select().from(users).where(eq(users.email, email)).limit(1))[0] ?? null
    },
    async createUser(input) {
      return (await db.insert(users).values(input).returning())[0]
    }
  }
}

export async function bootstrapAdminFromEnv() {
  const config = loadConfigFromEnv()
  const { email, password } = config.bootstrapAdmin
  if (!email || !password) return { created: false, skipped: true }

  const { db, pool } = createDb(config.databaseUrl)
  try {
    const service = createBootstrapAdminService({ repo: createBootstrapAdminRepository(db) })
    return await service.bootstrap({ email, password, now: new Date() })
  } finally {
    await pool.end()
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await bootstrapAdminFromEnv()
}
