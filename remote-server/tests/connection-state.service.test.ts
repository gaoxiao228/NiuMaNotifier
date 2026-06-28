import { describe, expect, it } from 'vitest'
import {
  createConnectionStateService,
  type ConnectionStateRedis
} from '../src/modules/connections/connection-state.service.js'

function createFakeRedis(): ConnectionStateRedis {
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

describe('connection state service', () => {
  it('stores and reads short-lived negotiation state', async () => {
    const service = createConnectionStateService({ redis: createFakeRedis(), ttlSeconds: 120 })
    await service.setPending({
      connectionId: 'conn_1',
      userId: 'usr_1',
      deviceId: 'dev_1',
      clientId: 'web_1',
      tokenHash: 'hash',
      status: 'signaling',
      createdAt: '2026-06-28T00:00:00.000Z',
      expiresAt: '2026-06-28T00:02:00.000Z'
    })

    await expect(service.get('conn_1')).resolves.toMatchObject({
      connection_id: 'conn_1',
      user_id: 'usr_1',
      device_id: 'dev_1',
      client_id: 'web_1',
      status: 'signaling'
    })
  })
})
