export type PresenceRecord = {
  user_id: string
  device_id: string
  socket_id: string
  server_instance_id: string
  last_seen_at: string
  capabilities: unknown
}

export type MarkOnlineInput = {
  userId: string
  deviceId: string
  socketId: string
  serverInstanceId: string
  lastSeenAt: string
  capabilities: unknown
}

export type PresenceRedis = {
  set(key: string, value: string, mode: 'EX', ttlSeconds: number): Promise<unknown>
  get(key: string): Promise<string | null>
  del(key: string): Promise<unknown>
}

function presenceKey(deviceId: string) {
  return `presence:device:${deviceId}`
}

export function createPresenceService(options: { redis: PresenceRedis; ttlSeconds: number }) {
  return {
    async markOnline(input: MarkOnlineInput) {
      const value: PresenceRecord = {
        user_id: input.userId,
        device_id: input.deviceId,
        socket_id: input.socketId,
        server_instance_id: input.serverInstanceId,
        last_seen_at: input.lastSeenAt,
        capabilities: input.capabilities
      }

      await options.redis.set(presenceKey(input.deviceId), JSON.stringify(value), 'EX', options.ttlSeconds)
    },

    async getPresence(deviceId: string): Promise<PresenceRecord | null> {
      const value = await options.redis.get(presenceKey(deviceId))
      return value ? (JSON.parse(value) as PresenceRecord) : null
    },

    async markOffline(deviceId: string) {
      await options.redis.del(presenceKey(deviceId))
    }
  }
}
