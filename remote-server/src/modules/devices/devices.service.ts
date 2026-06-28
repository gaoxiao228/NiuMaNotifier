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

export function createDevicesService(options: { repo: DevicesRepository }) {
  return {
    async list(userId: string): Promise<{ list: DeviceListItem[] }> {
      const devices = await options.repo.listActiveDevices(userId)

      return {
        list: devices.map((device) => ({
          id: device.id,
          name: device.name,
          // Redis presence 尚未接入；后续设备心跳阶段会把该字段改为真实状态。
          online: false,
          last_seen_at: device.lastSeenAt?.toISOString() ?? null,
          capabilities: device.capabilityJson
        }))
      }
    }
  }
}
