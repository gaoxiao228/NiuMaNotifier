import { and, eq, isNull } from 'drizzle-orm'
import { devices } from '../../db/schema.js'
import type { DeviceTokenRepository } from './device-token.service.js'
import type { DevicesRepository } from './devices.service.js'

export function createDevicesRepository(db: any): DevicesRepository & DeviceTokenRepository {
  return {
    async listActiveDevices(userId) {
      return db.select().from(devices).where(and(eq(devices.userId, userId), eq(devices.status, 'active')))
    },
    async findActiveDeviceByTokenHash(tokenHash) {
      const row = (
        await db
          .select()
          .from(devices)
          .where(and(eq(devices.tokenHash, tokenHash), eq(devices.status, 'active'), isNull(devices.revokedAt)))
          .limit(1)
      )[0]
      return row ?? null
    },
    async updateLastSeen(deviceId, lastSeenAt, capabilities) {
      await db
        .update(devices)
        .set({ lastSeenAt, capabilityJson: capabilities, updatedAt: lastSeenAt })
        .where(eq(devices.id, deviceId))
    },
    async revokeDeviceToken(userId, deviceId, revokedAt) {
      await db
        .update(devices)
        .set({ status: 'revoked', revokedAt, updatedAt: revokedAt })
        .where(and(eq(devices.userId, userId), eq(devices.id, deviceId)))
    }
  }
}
