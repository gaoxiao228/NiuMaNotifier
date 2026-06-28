import { describe, expect, it } from 'vitest'
import { createPresenceService, type PresenceRedis } from '../src/modules/devices/presence.service.js'

function createFakeRedis(): PresenceRedis {
  const values = new Map<string, string>()

  return {
    async set(key, value, mode, ttlSeconds) {
      expect(mode).toBe('EX')
      expect(ttlSeconds).toBe(90)
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

describe('presence service', () => {
  it('writes, reads, and deletes device presence', async () => {
    const service = createPresenceService({
      redis: createFakeRedis(),
      ttlSeconds: 90
    })

    await service.markOnline({
      userId: 'usr_1',
      deviceId: 'dev_1',
      socketId: 'sock_1',
      serverInstanceId: 'srv_1',
      lastSeenAt: '2026-06-28T00:00:00.000Z',
      capabilities: { supports_webrtc: true }
    })

    await expect(service.getPresence('dev_1')).resolves.toMatchObject({
      user_id: 'usr_1',
      device_id: 'dev_1',
      socket_id: 'sock_1'
    })

    await service.markOffline('dev_1')
    await expect(service.getPresence('dev_1')).resolves.toBeNull()
  })
})
