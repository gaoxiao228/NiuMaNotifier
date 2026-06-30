export type RelayRoute = {
  connection_id: string
  client_socket_id: string | null
  device_socket_id: string | null
  server_instance_id: string
  started_at: string
}

export type RelayRouteInput = {
  connectionId: string
  clientSocketId: string | null
  deviceSocketId: string | null
  serverInstanceId: string
  startedAt: string
}

export type RelayRouteRedis = {
  set(key: string, value: string, mode: 'EX', ttlSeconds: number): Promise<unknown>
  get(key: string): Promise<string | null>
  del(key: string): Promise<unknown>
}

function relayKey(connectionId: string) {
  return `relay:${connectionId}`
}

export function createRelayRouteService(options: { redis: RelayRouteRedis; ttlSeconds: number }) {
  return {
    async setRoute(input: RelayRouteInput) {
      const route: RelayRoute = {
        connection_id: input.connectionId,
        client_socket_id: input.clientSocketId,
        device_socket_id: input.deviceSocketId,
        server_instance_id: input.serverInstanceId,
        started_at: input.startedAt
      }

      await options.redis.set(relayKey(input.connectionId), JSON.stringify(route), 'EX', options.ttlSeconds)
    },

    async getRoute(connectionId: string): Promise<RelayRoute | null> {
      const value = await options.redis.get(relayKey(connectionId))
      return value ? (JSON.parse(value) as RelayRoute) : null
    },

    async deleteRoute(connectionId: string) {
      await options.redis.del(relayKey(connectionId))
    }
  }
}
