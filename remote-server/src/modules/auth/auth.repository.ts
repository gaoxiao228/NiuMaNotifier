import { eq } from 'drizzle-orm'
import { refreshTokens, users } from '../../db/schema.js'
import type { AuthRepository, AuthUser, RefreshTokenRecord } from './auth.service.js'

export function createAuthRepository(db: any): AuthRepository {
  return {
    async findUserByEmail(email) {
      const row = (await db.select().from(users).where(eq(users.email, email)).limit(1))[0]
      return (row as AuthUser | undefined) ?? null
    },
    async findUserById(id) {
      const row = (await db.select().from(users).where(eq(users.id, id)).limit(1))[0]
      return (row as AuthUser | undefined) ?? null
    },
    async createUser(input) {
      const row = (await db.insert(users).values(input).returning())[0]
      return row as AuthUser
    },
    async createRefreshToken(input) {
      const row = (await db.insert(refreshTokens).values(input).returning())[0]
      return row as RefreshTokenRecord
    },
    async findRefreshTokenByHash(tokenHash) {
      const row = (await db.select().from(refreshTokens).where(eq(refreshTokens.tokenHash, tokenHash)).limit(1))[0]
      return (row as RefreshTokenRecord | undefined) ?? null
    },
    async revokeRefreshToken(id) {
      await db.update(refreshTokens).set({ revokedAt: new Date() }).where(eq(refreshTokens.id, id))
    },
    async revokeAllRefreshTokens(userId) {
      await db.update(refreshTokens).set({ revokedAt: new Date() }).where(eq(refreshTokens.userId, userId))
    }
  }
}
