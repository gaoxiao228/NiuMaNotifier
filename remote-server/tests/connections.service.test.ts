import { describe, expect, it } from 'vitest'
import {
  createConnectionsService,
  type ConnectionsRepository
} from '../src/modules/connections/connections.service.js'
import { ErrorCode } from '../src/shared/errors.js'

function createRepo(): ConnectionsRepository {
  return {
    async findDeviceForUser(userId, deviceId) {
      if (userId !== 'usr_1' || deviceId !== 'dev_1') return null
      return { id: 'dev_1', userId: 'usr_1', name: 'NiuMa MacBook', status: 'active' }
    },
    async createConnection(input) {
      return { id: input.id, ...input }
    }
  }
}

describe('connections service', () => {
  it('creates connection for online device', async () => {
    const stateWrites: any[] = []
    const service = createConnectionsService({
      repo: createRepo(),
      presence: {
        async getPresence() {
          return {
            user_id: 'usr_1',
            device_id: 'dev_1',
            socket_id: 'sock_1',
            server_instance_id: 'srv_1',
            last_seen_at: '2026-06-28T00:00:00.000Z',
            capabilities: {}
          }
        }
      },
      state: {
        async setPending(input) {
          stateWrites.push(input)
        }
      },
      tokenPepper: 'pepper',
      publicUrl: 'https://remote.example.com',
      ttlSeconds: 120
    })

    const result = await service.create({
      userId: 'usr_1',
      deviceId: 'dev_1',
      clientId: 'web_1',
      transportPreference: 'webrtc_first'
    })

    expect(result.ok).toBe(true)
    if (!result.ok) throw new Error('connection create failed')
    expect(result.data.connection_id).toMatch(/^conn_/)
    expect(result.data.connection_token).toMatch(/^cnt_/)
    expect(result.data.signaling_url).toBe('wss://remote.example.com/ws/client')
    expect(stateWrites).toHaveLength(1)
  })

  it('rejects offline device as business failure', async () => {
    const service = createConnectionsService({
      repo: createRepo(),
      presence: { async getPresence() { return null } },
      state: { async setPending() {} },
      tokenPepper: 'pepper',
      publicUrl: 'https://remote.example.com',
      ttlSeconds: 120
    })

    await expect(
      service.create({
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1',
        transportPreference: 'webrtc_first'
      })
    ).resolves.toEqual({
      ok: false,
      code: ErrorCode.DEVICE_OFFLINE,
      message: '设备离线'
    })
  })
})
