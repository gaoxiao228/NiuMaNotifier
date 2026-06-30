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
  capabilities?: unknown
}

export type PresenceRedis = {
  set(key: string, value: string, mode: 'EX', ttlSeconds: number): Promise<unknown>
  get(key: string): Promise<string | null>
  del(key: string): Promise<unknown>
  eval(script: string, numKeys: number, ...args: Array<string | number>): Promise<unknown>
}

function presenceKey(deviceId: string) {
  return `presence:device:${deviceId}`
}

export function createPresenceService(options: { redis: PresenceRedis; ttlSeconds: number }) {
  return {
    async markOnline(input: MarkOnlineInput) {
      const existing = input.capabilities === undefined ? await this.getPresence(input.deviceId) : null
      const value: PresenceRecord = {
        user_id: input.userId,
        device_id: input.deviceId,
        socket_id: input.socketId,
        server_instance_id: input.serverInstanceId,
        last_seen_at: input.lastSeenAt,
        capabilities: input.capabilities ?? existing?.capabilities ?? {}
      }

      await options.redis.set(presenceKey(input.deviceId), JSON.stringify(value), 'EX', options.ttlSeconds)
    },

    async getPresence(deviceId: string): Promise<PresenceRecord | null> {
      const value = await options.redis.get(presenceKey(deviceId))
      return value ? (JSON.parse(value) as PresenceRecord) : null
    },

    async markOffline(deviceId: string, socketId?: string) {
      if (!socketId) {
        await options.redis.del(presenceKey(deviceId))
        return
      }

      // 设备快速重连时，旧 socket 的 close 事件可能晚于新 socket 的 hello；
      // 用 Redis Lua 原子校验 socket_id，避免旧连接误删新连接的在线状态。
      await options.redis.eval(
        `
local current = redis.call("GET", KEYS[1])
if not current then
  return 0
end
local ok, decoded = pcall(cjson.decode, current)
if not ok then
  return 0
end
if decoded["socket_id"] == ARGV[1] then
  return redis.call("DEL", KEYS[1])
end
return 0
        `,
        1,
        presenceKey(deviceId),
        socketId
      )
    }
  }
}
