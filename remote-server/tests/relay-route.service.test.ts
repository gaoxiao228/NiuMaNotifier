import { describe, expect, it } from 'vitest'
import { createRelayRouteService, type RelayRouteRedis } from '../src/modules/relay/relay-route.service.js'
import { relayBindSchema, relayFrameSchema } from '../src/modules/relay/relay.schemas.js'

function createFakeRedis(): RelayRouteRedis {
  const values = new Map<string, string>()

  return {
    async set(key, value, mode, ttlSeconds) {
      expect(mode).toBe('EX')
      expect(ttlSeconds).toBe(120)
      values.set(key, value)
      return 'OK'
    },
    async get(key) {
      return values.get(key) ?? null
    },
    async del(key) {
      values.delete(key)
      return 1
    }
  }
}

describe('relay schemas and route state', () => {
  it('validates bind query and ciphertext frame', () => {
    expect(
      relayBindSchema.parse({
        connection_id: 'conn_1',
        connection_token: 'cnt_token_with_enough_length_123456',
        side: 'client'
      }).side
    ).toBe('client')

    expect(
      relayFrameSchema.parse({
        version: 1,
        type: 'relay.frame',
        id: 'msg_1',
        connection_id: 'conn_1',
        seq: 1,
        ciphertext: 'YWJjZA=='
      }).ciphertext
    ).toBe('YWJjZA==')
  })

  it('writes and deletes relay route state', async () => {
    const service = createRelayRouteService({ redis: createFakeRedis(), ttlSeconds: 120 })
    await service.setRoute({
      connectionId: 'conn_1',
      clientSocketId: 'sock_client',
      deviceSocketId: 'sock_device',
      serverInstanceId: 'srv_1',
      startedAt: '2026-06-28T00:00:00.000Z'
    })

    await expect(service.getRoute('conn_1')).resolves.toEqual({
      connection_id: 'conn_1',
      client_socket_id: 'sock_client',
      device_socket_id: 'sock_device',
      server_instance_id: 'srv_1',
      started_at: '2026-06-28T00:00:00.000Z'
    })

    await service.deleteRoute('conn_1')
    await expect(service.getRoute('conn_1')).resolves.toBeNull()
  })
})
