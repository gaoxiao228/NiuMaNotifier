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
    },
    async eval(_script, _numKeys, key, socketId) {
      const value = typeof key === 'string' ? values.get(key) : null
      if (!value || typeof socketId !== 'string') return 0
      const record = JSON.parse(value) as { socket_id?: string }
      if (record.socket_id !== socketId) return 0
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

  it('does not let a stale socket close remove a newer presence record', async () => {
    const service = createPresenceService({
      redis: createFakeRedis(),
      ttlSeconds: 90
    })

    await service.markOnline({
      userId: 'usr_1',
      deviceId: 'dev_1',
      socketId: 'sock_old',
      serverInstanceId: 'srv_1',
      lastSeenAt: '2026-06-28T00:00:00.000Z',
      capabilities: { supports_webrtc: true }
    })
    await service.markOnline({
      userId: 'usr_1',
      deviceId: 'dev_1',
      socketId: 'sock_new',
      serverInstanceId: 'srv_1',
      lastSeenAt: '2026-06-28T00:00:10.000Z'
    })

    await service.markOffline('dev_1', 'sock_old')
    await expect(service.getPresence('dev_1')).resolves.toMatchObject({
      socket_id: 'sock_new'
    })

    await service.markOffline('dev_1', 'sock_new')
    await expect(service.getPresence('dev_1')).resolves.toBeNull()
  })

  it('preserves capabilities when refreshing existing presence without capabilities', async () => {
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
    await service.markOnline({
      userId: 'usr_1',
      deviceId: 'dev_1',
      socketId: 'sock_2',
      serverInstanceId: 'srv_1',
      lastSeenAt: '2026-06-28T00:00:20.000Z'
    })

    await expect(service.getPresence('dev_1')).resolves.toMatchObject({
      socket_id: 'sock_2',
      capabilities: { supports_webrtc: true }
    })
  })
})
