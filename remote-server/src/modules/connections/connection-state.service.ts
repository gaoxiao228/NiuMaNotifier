export type ConnectionState = {
  connection_id: string
  user_id: string
  device_id: string
  client_id: string
  token_hash: string
  transport_preference?: 'webrtc_first' | 'relay_first' | 'relay_only'
  status: 'pending' | 'signaling' | 'connected' | 'closed' | 'expired' | 'failed'
  created_at: string
  expires_at: string
}

export type SetConnectionStateInput = {
  connectionId: string
  userId: string
  deviceId: string
  clientId: string
  tokenHash: string
  transportPreference: 'webrtc_first' | 'relay_first' | 'relay_only'
  status: ConnectionState['status']
  createdAt: string
  expiresAt: string
}

export type ConnectionStateRedis = {
  set(key: string, value: string, mode: 'EX', ttlSeconds: number): Promise<unknown>
  get(key: string): Promise<string | null>
  del(key: string): Promise<unknown>
}

function connectionKey(connectionId: string) {
  return `connection:${connectionId}`
}

export function createConnectionStateService(options: {
  redis: ConnectionStateRedis
  ttlSeconds: number
}) {
  return {
    async setPending(input: SetConnectionStateInput) {
      const state: ConnectionState = {
        connection_id: input.connectionId,
        user_id: input.userId,
        device_id: input.deviceId,
        client_id: input.clientId,
        token_hash: input.tokenHash,
        transport_preference: input.transportPreference,
        status: input.status,
        created_at: input.createdAt,
        expires_at: input.expiresAt
      }

      await options.redis.set(connectionKey(input.connectionId), JSON.stringify(state), 'EX', options.ttlSeconds)
    },

    async get(connectionId: string): Promise<ConnectionState | null> {
      const value = await options.redis.get(connectionKey(connectionId))
      return value ? (JSON.parse(value) as ConnectionState) : null
    },

    async delete(connectionId: string) {
      await options.redis.del(connectionKey(connectionId))
    }
  }
}
