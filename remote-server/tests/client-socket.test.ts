import { describe, expect, it, vi } from 'vitest'
import { clientSignalMessageSchema } from '../src/modules/connections/connections.schemas.js'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'

describe('client signaling prerequisites', () => {
  it('sends messages to registered device socket', () => {
    const registry = createDeviceSocketRegistry()
    const send = vi.fn()
    registry.add('dev_1', { close: vi.fn(), send })

    expect(registry.sendToDevice('dev_1', { type: 'signal.offer' })).toBe(true)
    expect(send).toHaveBeenCalledWith(JSON.stringify({ type: 'signal.offer' }))
  })

  it('validates signaling messages', () => {
    expect(
      clientSignalMessageSchema.parse({
        version: 1,
        id: 'msg_1',
        type: 'signal.offer',
        data: { sdp: 'offer-sdp' }
      }).type
    ).toBe('signal.offer')
  })
})
