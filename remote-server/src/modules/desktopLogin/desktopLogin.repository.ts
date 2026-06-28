import { and, eq } from 'drizzle-orm'
import { desktopLoginSessions, devices } from '../../db/schema.js'
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
    async findActiveDeviceByFingerprint(userId, fingerprintHash) {
      const row = (
        await db
          .select()
          .from(devices)
          .where(
            and(
              eq(devices.userId, userId),
              eq(devices.fingerprintHash, fingerprintHash),
              eq(devices.status, 'active')
            )
          )
          .limit(1)
      )[0]
      return (row as { id: string; name: string } | undefined) ?? null
    },
    async upsertDevice(input) {
      const existing = await this.findActiveDeviceByFingerprint(input.userId, input.fingerprintHash)
      if (existing) {
        const row = (await db.update(devices).set(input).where(eq(devices.id, existing.id)).returning())[0]
        return row as { id: string; name: string }
      }

      const row = (await db.insert(devices).values(input).returning())[0]
      return row as { id: string; name: string }
    }
  }
}
