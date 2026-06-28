import { describe, expect, it, vi } from 'vitest'
import { createRelaySocketRegistry } from '../src/modules/relay/relay-socket-registry.js'

describe('relay socket registry', () => {
  it('forwards ciphertext frame to the opposite side', () => {
    const registry = createRelaySocketRegistry()
    const clientSend = vi.fn()
    const deviceSend = vi.fn()

    registry.add({
      connectionId: 'conn_1',
      side: 'client',
      socketId: 'sock_client',
      socket: { send: clientSend, close: vi.fn() }
    })
    registry.add({
      connectionId: 'conn_1',
      side: 'device',
      socketId: 'sock_device',
      socket: { send: deviceSend, close: vi.fn() }
    })

    expect(registry.forward('conn_1', 'client', { type: 'relay.frame', ciphertext: 'abc' })).toBe(true)
    expect(deviceSend).toHaveBeenCalledWith(JSON.stringify({ type: 'relay.frame', ciphertext: 'abc' }))
    expect(clientSend).not.toHaveBeenCalled()
  })

  it('tracks monotonic sequence per side', () => {
    const registry = createRelaySocketRegistry()

    expect(registry.acceptSeq('conn_1', 'client', 1)).toBe(true)
    expect(registry.acceptSeq('conn_1', 'client', 2)).toBe(true)
    expect(registry.acceptSeq('conn_1', 'client', 2)).toBe(false)
  })
})
