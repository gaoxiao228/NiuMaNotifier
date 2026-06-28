import { describe, expect, it, vi } from 'vitest'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'
import { createDevicesService } from '../src/modules/devices/devices.service.js'

describe('device list presence merge and revoke token', () => {
  it('marks devices online when Redis presence exists', async () => {
    const service = createDevicesService({
      repo: {
        async listActiveDevices() {
          return [
            {
              id: 'dev_1',
              name: 'NiuMa MacBook',
              lastSeenAt: new Date('2026-06-28T00:00:00.000Z'),
              capabilityJson: { supports_webrtc: true }
            }
          ]
        }
      },
      presence: {
        async getPresence(deviceId: string) {
          return deviceId === 'dev_1'
            ? {
                user_id: 'usr_1',
                device_id: 'dev_1',
                socket_id: 'sock_1',
                server_instance_id: 'srv_1',
                last_seen_at: '2026-06-28T00:01:00.000Z',
                capabilities: { supports_webrtc: true }
              }
            : null
        }
      }
    })

    await expect(service.list('usr_1')).resolves.toEqual({
      list: [
        {
          id: 'dev_1',
          name: 'NiuMa MacBook',
          online: true,
          last_seen_at: '2026-06-28T00:01:00.000Z',
          capabilities: { supports_webrtc: true }
        }
      ]
    })
  })

  it('closes online socket when token is revoked', async () => {
    const registry = createDeviceSocketRegistry()
    const close = vi.fn()
    registry.add('dev_1', { close })

    registry.closeDevice('dev_1', 4003, 'token_revoked')

    expect(close).toHaveBeenCalledWith(4003, 'token_revoked')
  })
})
