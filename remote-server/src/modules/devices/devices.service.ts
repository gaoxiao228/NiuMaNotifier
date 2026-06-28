import type { PresenceRecord } from './presence.service.js'

export type DeviceListItem = {
  id: string
  name: string
  online: boolean
  last_seen_at: string | null
  capabilities: unknown
}

export type DevicesRepository = {
  listActiveDevices(userId: string): Promise<
    Array<{
      id: string
      name: string
      lastSeenAt: Date | null
      capabilityJson: unknown
    }>
  >
  revokeDeviceToken?(userId: string, deviceId: string, revokedAt: Date): Promise<void>
}

export type DevicePresenceReader = {
  getPresence(deviceId: string): Promise<PresenceRecord | null>
  markOffline?(deviceId: string): Promise<void>
}

export function createDevicesService(options: { repo: DevicesRepository; presence?: DevicePresenceReader }) {
  return {
    async list(userId: string): Promise<{ list: DeviceListItem[] }> {
      const devices = await options.repo.listActiveDevices(userId)
      const list = await Promise.all(
        devices.map(async (device) => {
          const presence = options.presence ? await options.presence.getPresence(device.id) : null

          return {
            id: device.id,
            name: device.name,
            online: Boolean(presence),
            last_seen_at: presence?.last_seen_at ?? device.lastSeenAt?.toISOString() ?? null,
            capabilities: presence?.capabilities ?? device.capabilityJson
          }
        })
      )

      return { list }
    },

    async revokeToken(input: { userId: string; deviceId: string; now: Date }) {
      if (!options.repo.revokeDeviceToken) throw new Error('revokeDeviceToken not implemented')

      await options.repo.revokeDeviceToken(input.userId, input.deviceId, input.now)
      if (options.presence?.markOffline) await options.presence.markOffline(input.deviceId)
      return {}
    }
  }
}
