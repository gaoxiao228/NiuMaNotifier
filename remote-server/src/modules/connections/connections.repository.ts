import { and, eq } from 'drizzle-orm'
import { devices, remoteConnections } from '../../db/schema.js'
import type { ConnectionsRepository } from './connections.service.js'

export function createConnectionsRepository(db: any): ConnectionsRepository {
  return {
    async findDeviceForUser(userId, deviceId) {
      const row = (
        await db
          .select()
          .from(devices)
          .where(and(eq(devices.userId, userId), eq(devices.id, deviceId), eq(devices.status, 'active')))
          .limit(1)
      )[0]
      return row ?? null
    },

    async createConnection(input) {
      const row = (await db.insert(remoteConnections).values(input).returning())[0]
      return row as { id: string }
    }
  }
}
