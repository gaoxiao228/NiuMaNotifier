import { eq, sql } from 'drizzle-orm'
import { desktopLoginSessions } from '../../db/schema.js'
import type { DesktopLoginRepository, DesktopLoginSession } from './desktopLogin.service.js'

export function createDesktopLoginRepository(db: any): DesktopLoginRepository {
  return {
    async createSession(input) {
      const row = (await db.insert(desktopLoginSessions).values(input).returning())[0]
      return row as DesktopLoginSession
    },
    async findSessionByRequestId(requestId) {
      const row = (
        await db
          .select()
          .from(desktopLoginSessions)
          .where(eq(desktopLoginSessions.requestId, requestId))
          .limit(1)
      )[0]
      return (row as DesktopLoginSession | undefined) ?? null
    },
    async completeSession(requestId, input) {
      await db
        .update(desktopLoginSessions)
        .set(input)
        .where(eq(desktopLoginSessions.requestId, requestId))
    },
    async consumeSession(requestId) {
      await db
        .update(desktopLoginSessions)
        .set({ status: 'consumed', consumedAt: new Date() })
        .where(eq(desktopLoginSessions.requestId, requestId))
    },
    async upsertDevice(input) {
      // PostgreSQL 的部分唯一索引需要用原生 ON CONFLICT 匹配 predicate，避免先查再写竞态。
      const result = await db.execute(sql`
        INSERT INTO devices (
          user_id,
          name,
          fingerprint_hash,
          token_hash,
          identity_public_key_json,
          status,
          capability_json,
          created_at,
          updated_at,
          revoked_at
        )
        VALUES (
          ${input.userId},
          ${input.name},
          ${input.fingerprintHash},
          ${input.tokenHash},
          ${JSON.stringify(input.identityPublicKeyJson)}::jsonb,
          ${input.status},
          ${JSON.stringify(input.capabilityJson)}::jsonb,
          ${input.createdAt},
          ${input.updatedAt},
          ${input.revokedAt}
        )
        ON CONFLICT (user_id, fingerprint_hash)
        WHERE "status" = 'active'
        DO UPDATE SET
          name = EXCLUDED.name,
          token_hash = EXCLUDED.token_hash,
          identity_public_key_json = EXCLUDED.identity_public_key_json,
          capability_json = EXCLUDED.capability_json,
          updated_at = EXCLUDED.updated_at,
          revoked_at = NULL
        RETURNING id, name
      `)
      const row = result.rows[0]
      return row as { id: string; name: string }
    }
  }
}
