import { and, eq } from 'drizzle-orm'
import { devices } from '../../db/schema.js'
import type { DevicesRepository } from './devices.service.js'

export function createDevicesRepository(db: any): DevicesRepository {
  return {
    async listActiveDevices(userId) {
      return db.select().from(devices).where(and(eq(devices.userId, userId), eq(devices.status, 'active')))
    }
  }
}
