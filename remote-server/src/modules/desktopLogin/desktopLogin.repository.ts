import { and, eq, sql } from 'drizzle-orm'
import { desktopLoginSessions } from '../../db/schema.js'
import type { DesktopLoginRepository, DesktopLoginSession } from './desktopLogin.service.js'

export function createDesktopLoginRepository(db: any): DesktopLoginRepository {
  return {
    async runDeviceBindingTransaction(fingerprintHash, action) {
      return db.transaction(async (tx: any) => {
        // 同一设备指纹的绑定完成流程必须串行化，避免 token 轮换后旧会话重新写回 completed。
        await tx.execute(sql`SELECT pg_advisory_xact_lock(hashtext(${fingerprintHash}))`)
        return action(createDesktopLoginRepository(tx))
      })
    },
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
      const rows = await db
        .update(desktopLoginSessions)
        .set(input)
        .where(
          and(
            eq(desktopLoginSessions.requestId, requestId),
            eq(desktopLoginSessions.status, 'pending')
          )
        )
        .returning({ requestId: desktopLoginSessions.requestId })
      return rows.length > 0
    },
    async consumeSession(requestId) {
      await db
        .update(desktopLoginSessions)
        .set({ status: 'consumed', consumedAt: new Date() })
        .where(eq(desktopLoginSessions.requestId, requestId))
    },
    async consumeOtherSessionsForDevice(input) {
      // 新 complete 会轮换设备 token，旧会话必须被消费，避免之后 poll 返回失效凭据。
      await db.execute(sql`
        UPDATE desktop_login_sessions
        SET status = 'consumed', consumed_at = ${input.consumedAt}
        WHERE fingerprint_hash = ${input.fingerprintHash}
          AND request_id <> ${input.requestId}
          AND (
            (status = 'pending' AND created_at < ${input.createdBefore})
            OR (status = 'completed' AND user_id = ${input.userId})
          )
      `)
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
