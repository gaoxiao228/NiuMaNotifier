import { describe, expect, it, vi } from 'vitest'

import { createRemoteMessageBus, type RemoteTransport } from '../remote/remoteTransport.js'

describe('createRemoteMessageBus', () => {
  function transport(kind: 'relay' | 'webrtc') {
    return {
      kind,
      send: vi.fn(),
      close: vi.fn()
    } satisfies RemoteTransport
  }

  it('sends through relay when it is the only open transport', () => {
    const relay = transport('relay')
    const bus = createRemoteMessageBus({ onInbound: vi.fn() })

    bus.register(relay)
    bus.setOpen('relay', true)
    bus.send({ type: 'request', id: 'rpc_1' })

    expect(relay.send).toHaveBeenCalledWith({ type: 'request', id: 'rpc_1' })
  })

  it('prefers webrtc when relay and webrtc are both open', () => {
    const relay = transport('relay')
    const webrtc = transport('webrtc')
    const bus = createRemoteMessageBus({ onInbound: vi.fn() })

    bus.register(relay)
    bus.register(webrtc)
    bus.setOpen('relay', true)
    bus.setOpen('webrtc', true)
    bus.send({ type: 'request', id: 'rpc_2' })

    expect(webrtc.send).toHaveBeenCalledWith({ type: 'request', id: 'rpc_2' })
    expect(relay.send).not.toHaveBeenCalled()
  })

  it('marks the outgoing payload with the selected transport without mutating the original request', () => {
    const relay = transport('relay')
    const webrtc = transport('webrtc')
    const bus = createRemoteMessageBus({ onInbound: vi.fn() })
    const request = {
      version: 1,
      type: 'request',
      id: 'rpc_3',
      transport: { kind: 'relay' },
      method: 'rpc.ping',
      params: {}
    }

    bus.register(relay)
    bus.register(webrtc)
    bus.setOpen('relay', true)
    bus.setOpen('webrtc', true)
    bus.send(request)

    expect(webrtc.send).toHaveBeenCalledWith(
      expect.objectContaining({
        transport: { kind: 'webrtc' }
      })
    )
    expect(request.transport.kind).toBe('relay')
  })

  it('wraps incoming payloads with the observed transport', () => {
    const onInbound = vi.fn()
    const bus = createRemoteMessageBus({ onInbound })

    bus.receive({ type: 'notification' }, 'relay')

    expect(onInbound).toHaveBeenCalledWith({
      payload: { type: 'notification' },
      observedTransport: 'relay'
    })
  })

  it('throws when no transport is open', () => {
    const bus = createRemoteMessageBus({ onInbound: vi.fn() })

    expect(() => bus.send({ type: 'request' })).toThrow('No remote transport is open')
  })
})
