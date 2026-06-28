import { describe, expect, it, vi } from 'vitest'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'
import { deviceSocketMessageSchema } from '../src/ws/ws-message.schemas.js'

describe('device websocket schema and registry', () => {
  it('accepts hello and heartbeat messages', () => {
    expect(
      deviceSocketMessageSchema.parse({
        version: 1,
        type: 'device.hello',
        id: 'msg_1',
        data: {
          device_id: 'dev_1',
          agent_protocol_version: 1,
          rpc_protocol_version: 1,
          capabilities: { supports_webrtc: true }
        }
      }).type
    ).toBe('device.hello')

    expect(
      deviceSocketMessageSchema.parse({
        version: 1,
        type: 'device.heartbeat',
        id: 'msg_2',
        data: {}
      }).type
    ).toBe('device.heartbeat')
  })

  it('closes a registered socket when device is revoked', () => {
    const registry = createDeviceSocketRegistry()
    const close = vi.fn()

    registry.add('dev_1', { close })
    expect(registry.has('dev_1')).toBe(true)
    registry.closeDevice('dev_1', 4003, 'token_revoked')

    expect(close).toHaveBeenCalledWith(4003, 'token_revoked')
    expect(registry.has('dev_1')).toBe(false)
  })
})
