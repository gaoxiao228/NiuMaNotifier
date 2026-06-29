import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { RemoteDevice } from '../api/devicesApi.js'
import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import { DeviceConsolePage } from '../remote/deviceConsolePage.js'
import type { ConnectionClientOptions, ConnectionStatus } from '../remote/connectionClient.js'
import type { RelayClient, RelayClientOptions } from '../remote/relayTransport.js'
import type { WebRtcTransport, WebRtcTransportOptions } from '../remote/webrtcTransport.js'
import { createTranslator } from '../i18n/index.js'

const t = createTranslator('en')

function createDevice(online: boolean): RemoteDevice {
  return {
    id: 'device-1',
    name: 'Desk Mac',
    online,
    last_seen_at: null,
    capabilities: {},
    identity_public_key: {}
  }
}

function createConnectionResult(): ConnectionCreateResult {
  return createConnectionResultWithId('conn_123')
}

function createConnectionResultWithId(connectionId: string): ConnectionCreateResult {
  return {
    connection_id: connectionId,
    connection_token: 'short_token',
    expires_at: '2026-06-29T00:00:00Z',
    expires_in: 60,
    signaling_url: 'ws://127.0.0.1:27880/ws/client',
    relay_url: 'https://relay.example.com'
  }
}

function openReadyRelay(options: RelayClientOptions | null) {
  options?.onOpen()
  options?.onReady()
}

async function openRelayConsoleWithSessionPayload(sessionPayload: unknown) {
  const create = vi.fn().mockResolvedValue(createConnectionResult())
  const signalClients: Array<{ onStatus: (status: ConnectionStatus) => void }> = []
  const createConnection = vi.fn((options: ConnectionClientOptions) => {
    const client = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: options.onStatus,
      onMessage: options.onMessage
    }
    signalClients.push(client)
    return client
  })
  let relayOptions: RelayClientOptions | null = null
  const relayClient: RelayClient = {
    socket: {} as WebSocket,
    send: vi.fn(),
    close: vi.fn()
  }
  const createRelay = vi.fn((options: RelayClientOptions) => {
    relayOptions = options
    return relayClient
  })

  render(
    <DeviceConsolePage
      device={createDevice(true)}
      connectionsApi={{ create }}
      createConnection={createConnection}
      createRelay={createRelay}
      t={t}
      onBack={() => {}}
    />
  )

  fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
  await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

  act(() => {
    signalClients[0]?.onStatus('accepted')
  })
  await waitFor(() => expect(createRelay).toHaveBeenCalledTimes(1))

  await act(async () => {
    openReadyRelay(relayOptions)
    relayOptions?.onPayload({ version: 1, type: 'response', id: 'rpc_3', ok: true, result: { stream_id: 'stream_1' } })
    await Promise.resolve()
    relayOptions?.onPayload({
      version: 1,
      type: 'notification',
      method: 'local_api.stream.event',
      params: {
        stream_id: 'stream_1',
        event: 'session_project_groups',
        id: '1',
        data: sessionPayload
      }
    })
    await Promise.resolve()
  })

  return { relayClient }
}

afterEach(() => {
  cleanup()
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('DeviceConsolePage', () => {
  it('starts a connection automatically when requested for an online device', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const createConnection = vi.fn()

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        t={t}
        onBack={() => {}}
        autoConnect
      />
    )

    await waitFor(() => {
      expect(create).toHaveBeenCalledWith('device-1', expect.stringMatching(/^niuma-web-client-/))
      expect(createConnection).toHaveBeenCalledTimes(1)
    })
  })

  it('creates a relay-first connection and opens a websocket client for an online device', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const createConnection = vi.fn()

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))

    await waitFor(() => {
      expect(create).toHaveBeenCalledWith('device-1', expect.stringMatching(/^niuma-web-client-/))
      expect(createConnection).toHaveBeenCalledWith(
        expect.objectContaining({
          url: 'ws://127.0.0.1:27880/ws/client?connection_id=conn_123&connection_token=short_token'
        })
      )
    })
    expect(screen.getByText('conn_123')).not.toBeNull()
  })

  it('disables connection for an offline device and keeps the back entry available', () => {
    const onBack = vi.fn()

    render(
      <DeviceConsolePage
        device={createDevice(false)}
        connectionsApi={{ create: vi.fn() }}
        t={t}
        onBack={onBack}
      />
    )

    expect((screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Back to devices' }))

    expect(onBack).toHaveBeenCalled()
  })

  it('does not touch a localStorage accessor in the Node test environment', async () => {
    const descriptor = Object.getOwnPropertyDescriptor(window, 'localStorage')
    const localStorageGetter = vi.fn(() => {
      throw new Error('localStorage getter should not be touched in Node')
    })
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      get: localStorageGetter
    })
    const create = vi.fn().mockResolvedValue(createConnectionResult())

    try {
      render(
        <DeviceConsolePage
          device={createDevice(true)}
          connectionsApi={{ create }}
          createConnection={vi.fn()}
          t={t}
          onBack={() => {}}
        />
      )

      fireEvent.click(screen.getByRole('button', { name: 'Connect' }))

      await waitFor(() => expect(create).toHaveBeenCalled())
      expect(localStorageGetter).not.toHaveBeenCalled()
    } finally {
      if (descriptor) Object.defineProperty(window, 'localStorage', descriptor)
    }
  })

  it('ignores stale socket close, error, and messages after reconnecting', async () => {
    const create = vi
      .fn()
      .mockResolvedValueOnce(createConnectionResultWithId('conn_old'))
      .mockResolvedValueOnce(createConnectionResultWithId('conn_new'))
    const clients: Array<{
      socket: WebSocket
      close: ReturnType<typeof vi.fn>
      onStatus: (status: ConnectionStatus) => void
      onMessage: (value: unknown) => void
    }> = []
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      const client = {
        socket: {} as WebSocket,
        send: vi.fn(),
        close: vi.fn(),
        onStatus: options.onStatus,
        onMessage: options.onMessage
      }
      clients.push(client)
      return client
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))
    act(() => clients[0]?.onStatus('accepted'))

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(2))
    act(() => clients[1]?.onStatus('accepted'))
    act(() => {
      clients[0]?.onStatus('closed')
      clients[0]?.onStatus('error')
      clients[0]?.onMessage({ type: 'connection.reject', stale: true })
    })

    expect(clients[0]?.close).toHaveBeenCalledTimes(1)
    expect(screen.getByText('Accepted')).not.toBeNull()
    expect(screen.queryByText(/stale/)).toBeNull()
  })

  it('closes the current socket on unmount and ignores later callbacks', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const client = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      client.onStatus = options.onStatus
      client.onMessage = options.onMessage
      return client
    })

    const { unmount } = render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    unmount()

    expect(client.close).toHaveBeenCalledTimes(1)
    expect(() => {
      client.onStatus('closed')
      client.onStatus('error')
      client.onMessage({ type: 'connection.accept' })
    }).not.toThrow()
  })

  it('opens relay after accept, sends console RPC requests, and renders JSON responses', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClients: Array<{
      close: ReturnType<typeof vi.fn>
      onStatus: (status: ConnectionStatus) => void
      onMessage: (value: unknown) => void
    }> = []
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      const client = {
        socket: {} as WebSocket,
        send: vi.fn(),
        close: vi.fn(),
        onStatus: options.onStatus,
        onMessage: options.onMessage
      }
      signalClients.push(client)
      return client
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    act(() => {
      signalClients[0]?.onStatus('accepted')
      signalClients[0]?.onMessage({ type: 'connection.accept' })
    })

    await waitFor(() => {
      expect(createRelay).toHaveBeenCalledWith(
        expect.objectContaining({
          url: 'wss://relay.example.com/ws/relay?connection_id=conn_123&connection_token=short_token&side=client',
          connectionId: 'conn_123'
        })
      )
    })

    act(() => {
      relayOptions?.onOpen()
    })
    expect(relayClient.send).not.toHaveBeenCalled()

    act(() => {
      relayOptions?.onReady()
    })

    expect(relayClient.send).toHaveBeenCalledTimes(3)
    expect(relayClient.send).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ id: 'rpc_1', method: 'rpc.ping' })
    )
    expect(relayClient.send).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ transport: { kind: 'relay' } })
    )
    expect(relayClient.send).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ id: 'rpc_2', method: 'state.get' })
    )
    expect(relayClient.send).toHaveBeenNthCalledWith(
      3,
      expect.objectContaining({
        id: 'rpc_3',
        method: 'local_api.stream',
        params: {
          method: 'GET',
          path: '/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20',
          headers: {},
          body: null
        }
      })
    )

    await act(async () => {
      relayOptions?.onPayload({ version: 1, type: 'response', id: 'rpc_1', ok: true, result: { pong: true } })
      relayOptions?.onPayload({
        version: 1,
        type: 'response',
        id: 'rpc_2',
        ok: true,
        result: { state: 'ready' }
      })
      relayOptions?.onPayload({
        version: 1,
        type: 'response',
        id: 'rpc_3',
        ok: true,
        result: { stream_id: 'stream_1' }
      })
      await Promise.resolve()
      relayOptions?.onPayload({
        version: 1,
        type: 'notification',
        transport: {
          kind: 'relay'
        },
        method: 'local_api.stream.event',
        params: {
          stream_id: 'stream_1',
          seq: 1,
          event: 'session_project_groups',
          id: '1',
          data: {
            list: [
              {
                tool: 'codex',
                project_name: 'repo',
                project_path: '/repo',
                sessions: [
                  {
                    normalized_session_id: 'session-1',
                    primary_session_id: 's1',
                    title: 'Demo session',
                    runtime_status: 'running',
                    status: 'idle',
                    first_user_message_preview: 'Inspect remote sessions',
                    latest_event_summary: null,
                    subagent_count: 0
                  }
                ]
              }
            ],
            page: 1,
            page_size: 20,
            total: 1
          }
        }
      })
      await Promise.resolve()
    })

    expect(await screen.findByText('Ping')).not.toBeNull()
    expect(screen.getByText(/"pong": true/)).not.toBeNull()
    expect(screen.getByText(/"state": "ready"/)).not.toBeNull()
    expect(screen.getByText(/"project_name": "repo"/)).not.toBeNull()
    expect(screen.getByText(/"title": "Demo session"/)).not.toBeNull()
    expect(screen.getByText('repo')).not.toBeNull()
    expect(screen.getByText('/repo')).not.toBeNull()
    expect(screen.getByText('Demo session')).not.toBeNull()
    expect(screen.getByText('running')).not.toBeNull()
    expect(screen.getByText('Inspect remote sessions')).not.toBeNull()
  })

  it('starts WebRTC negotiation after relay accept and sends offer and ICE over signaling', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      void options
      return relayClient
    })
    const createWebRtc = vi.fn((options: WebRtcTransportOptions): WebRtcTransport => {
      return {
        kind: 'webrtc',
        start: vi.fn(async () => {
          options.onOffer({ connection_id: 'conn_123', sdp: 'offer-sdp' })
          options.onIceCandidate({
            connection_id: 'conn_123',
            candidate: 'candidate:1',
            sdp_mid: '0',
            sdp_mline_index: 0
          })
        }),
        acceptAnswer: vi.fn(),
        addRemoteIceCandidate: vi.fn(),
        send: vi.fn(),
        close: vi.fn()
      }
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        createWebRtc={createWebRtc}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    await act(async () => {
      signalClient.onStatus('accepted')
      await Promise.resolve()
    })

    await waitFor(() => expect(createWebRtc).toHaveBeenCalledTimes(1))
    expect(signalClient.send).toHaveBeenCalledWith(
      expect.objectContaining({
        version: 1,
        type: 'signal.offer',
        data: { sdp: 'offer-sdp' }
      })
    )
    expect(signalClient.send).toHaveBeenCalledWith(
      expect.objectContaining({
        version: 1,
        type: 'signal.ice_candidate',
        data: {
          candidate: 'candidate:1',
          sdp_mid: '0',
          sdp_mline_index: 0
        }
      })
    )
  })

  it('forwards signaling answer and ICE candidate messages to the WebRTC transport', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      void options
      return relayClient
    })
    const webRtcTransport: WebRtcTransport = {
      kind: 'webrtc',
      start: vi.fn(async () => {}),
      acceptAnswer: vi.fn(async () => {}),
      addRemoteIceCandidate: vi.fn(async () => {}),
      send: vi.fn(),
      close: vi.fn()
    }
    const createWebRtc = vi.fn(() => webRtcTransport)

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        createWebRtc={createWebRtc}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))
    await act(async () => {
      signalClient.onStatus('accepted')
      await Promise.resolve()
    })

    await act(async () => {
      signalClient.onMessage({
        version: 1,
        type: 'signal.answer',
        id: 'msg_answer',
        data: { connection_id: 'conn_123', sdp: 'answer-sdp' }
      })
      signalClient.onMessage({
        version: 1,
        type: 'signal.ice_candidate',
        id: 'msg_ice',
        data: {
          connection_id: 'conn_123',
          candidate: 'candidate:2',
          sdp_mid: null,
          sdp_mline_index: null
        }
      })
      await Promise.resolve()
    })

    expect(webRtcTransport.acceptAnswer).toHaveBeenCalledWith({
      connection_id: 'conn_123',
      sdp: 'answer-sdp'
    })
    expect(webRtcTransport.addRemoteIceCandidate).toHaveBeenCalledWith({
      connection_id: 'conn_123',
      candidate: 'candidate:2',
      sdp_mid: null,
      sdp_mline_index: null
    })
  })

  it('shows the active transport switching from relay to WebRTC and back to relay', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })
    let webRtcOptions: WebRtcTransportOptions | null = null
    const webRtcSend = vi.fn()
    const createWebRtc = vi.fn((options: WebRtcTransportOptions): WebRtcTransport => {
      webRtcOptions = options
      return {
        kind: 'webrtc',
        start: vi.fn(async () => {}),
        acceptAnswer: vi.fn(),
        addRemoteIceCandidate: vi.fn(),
        send: webRtcSend,
        close: vi.fn()
      }
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        createWebRtc={createWebRtc}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))
    await act(async () => {
      signalClient.onStatus('accepted')
      await Promise.resolve()
    })

    act(() => {
      openReadyRelay(relayOptions)
    })
    expect(screen.getByText('Active transport: Relay')).not.toBeNull()

    act(() => {
      webRtcOptions?.onOpen()
    })
    expect(screen.getByText('Active transport: Relay')).not.toBeNull()

    const probeRequest = webRtcSend.mock.calls[0]?.[0]
    expect(probeRequest).toEqual(
      expect.objectContaining({
        type: 'request',
        method: 'rpc.ping',
        transport: { kind: 'webrtc' }
      })
    )

    act(() => {
      webRtcOptions?.onPayload({
        version: 1,
        type: 'response',
        id: probeRequest.id,
        ok: true,
        result: { pong: true },
        transport: { kind: 'webrtc' }
      })
    })
    expect(screen.getByText('Active transport: WebRTC')).not.toBeNull()

    act(() => {
      webRtcOptions?.onClose()
    })
    expect(screen.getByText('Active transport: Relay')).not.toBeNull()
  })

  it('keeps relay active when WebRTC RPC probe fails', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })
    let webRtcOptions: WebRtcTransportOptions | null = null
    const webRtcSend = vi.fn()
    const webRtcClose = vi.fn()
    const createWebRtc = vi.fn((options: WebRtcTransportOptions): WebRtcTransport => {
      webRtcOptions = options
      return {
        kind: 'webrtc',
        start: vi.fn(async () => {}),
        acceptAnswer: vi.fn(),
        addRemoteIceCandidate: vi.fn(),
        send: webRtcSend,
        close: webRtcClose
      }
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        createWebRtc={createWebRtc}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))
    await act(async () => {
      signalClient.onStatus('accepted')
      await Promise.resolve()
    })

    act(() => {
      openReadyRelay(relayOptions)
      webRtcOptions?.onOpen()
    })

    const probeRequest = webRtcSend.mock.calls[0]?.[0]
    act(() => {
      webRtcOptions?.onPayload({
        version: 1,
        type: 'response',
        id: probeRequest.id,
        ok: false,
        error: { code: 'method_not_found', message: 'unknown RPC method: rpc.ping' },
        transport: { kind: 'webrtc' }
      })
    })

    expect(screen.getByText('WebRTC error')).not.toBeNull()
    expect(screen.getByText('Active transport: Relay')).not.toBeNull()
    expect(webRtcClose).toHaveBeenCalledTimes(1)
  })

  it('starts remote RPC reads when WebRTC becomes ready before relay', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    let webRtcOptions: WebRtcTransportOptions | null = null
    const webRtcSend = vi.fn()
    const createWebRtc = vi.fn((options: WebRtcTransportOptions): WebRtcTransport => {
      webRtcOptions = options
      return {
        kind: 'webrtc',
        start: vi.fn(async () => {}),
        acceptAnswer: vi.fn(),
        addRemoteIceCandidate: vi.fn(),
        send: webRtcSend,
        close: vi.fn()
      }
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createWebRtc={createWebRtc}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))
    await act(async () => {
      signalClient.onStatus('accepted')
      await Promise.resolve()
    })

    act(() => {
      webRtcOptions?.onOpen()
    })
    const probeRequest = webRtcSend.mock.calls[0]?.[0]
    act(() => {
      webRtcOptions?.onPayload({
        version: 1,
        type: 'response',
        id: probeRequest.id,
        ok: true,
        result: { pong: true },
        transport: { kind: 'webrtc' }
      })
    })

    expect(screen.getByText('Active transport: WebRTC')).not.toBeNull()
    expect(webRtcSend.mock.calls.map((call) => call[0]?.method)).toEqual([
      'rpc.ping',
      'rpc.ping',
      'state.get',
      'local_api.stream'
    ])
    expect(screen.getAllByText('Waiting for response')).toHaveLength(4)
  })

  it('falls back to relay when WebRTC RPC requests time out', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })
    let webRtcOptions: WebRtcTransportOptions | null = null
    const webRtcSend = vi.fn()
    const webRtcClose = vi.fn()
    const createWebRtc = vi.fn((options: WebRtcTransportOptions): WebRtcTransport => {
      webRtcOptions = options
      return {
        kind: 'webrtc',
        start: vi.fn(async () => {}),
        acceptAnswer: vi.fn(),
        addRemoteIceCandidate: vi.fn(),
        send: webRtcSend,
        close: webRtcClose
      }
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        createWebRtc={createWebRtc}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))
    await act(async () => {
      signalClient.onStatus('accepted')
      await Promise.resolve()
    })

    act(() => {
      webRtcOptions?.onOpen()
    })
    const probeRequest = webRtcSend.mock.calls[0]?.[0]
    vi.useFakeTimers()
    act(() => {
      webRtcOptions?.onPayload({
        version: 1,
        type: 'response',
        id: probeRequest.id,
        ok: true,
        result: { pong: true },
        transport: { kind: 'webrtc' }
      })
    })
    expect(screen.getByText('Active transport: WebRTC')).not.toBeNull()

    act(() => {
      openReadyRelay(relayOptions)
    })
    expect(webRtcSend.mock.calls.map((call) => call[0]?.method)).toEqual([
      'rpc.ping',
      'rpc.ping',
      'state.get',
      'local_api.stream'
    ])
    expect(relayClient.send).not.toHaveBeenCalled()

    await act(async () => {
      vi.advanceTimersByTime(10_000)
      await Promise.resolve()
      await Promise.resolve()
    })

    expect(screen.getByText('Active transport: Relay')).not.toBeNull()
    expect(webRtcClose).toHaveBeenCalledTimes(1)
    const relayRequests = vi
      .mocked(relayClient.send)
      .mock.calls.map((call) => call[0] as { method?: string; transport?: { kind?: string } })
    expect(relayRequests.map((request) => request.method)).toEqual([
      'rpc.ping',
      'state.get',
      'local_api.stream'
    ])
    expect(relayRequests.every((request) => request.transport?.kind === 'relay')).toBe(true)
  })

  it('closes pending RPC requests when relay closes first and WebRTC closes later', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })
    let webRtcOptions: WebRtcTransportOptions | null = null
    const webRtcSend = vi.fn()
    const createWebRtc = vi.fn((options: WebRtcTransportOptions): WebRtcTransport => {
      webRtcOptions = options
      return {
        kind: 'webrtc',
        start: vi.fn(async () => {}),
        acceptAnswer: vi.fn(),
        addRemoteIceCandidate: vi.fn(),
        send: webRtcSend,
        close: vi.fn()
      }
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        createWebRtc={createWebRtc}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))
    await act(async () => {
      signalClient.onStatus('accepted')
      await Promise.resolve()
    })

    act(() => {
      openReadyRelay(relayOptions)
      webRtcOptions?.onOpen()
    })
    const probeRequest = webRtcSend.mock.calls[0]?.[0]
    act(() => {
      webRtcOptions?.onPayload({
        version: 1,
        type: 'response',
        id: probeRequest.id,
        ok: true,
        result: { pong: true },
        transport: { kind: 'webrtc' }
      })
    })
    expect(screen.getByText('Active transport: WebRTC')).not.toBeNull()

    await act(async () => {
      relayOptions?.onClose()
      await Promise.resolve()
    })
    expect(screen.getByText('Relay closed')).not.toBeNull()
    expect(screen.getByText('Active transport: WebRTC')).not.toBeNull()

    await act(async () => {
      webRtcOptions?.onClose()
      await Promise.resolve()
    })

    expect(screen.getByText('WebRTC closed')).not.toBeNull()
    expect(screen.getByText('Active transport: Idle')).not.toBeNull()
    expect(screen.getAllByText('RPC request failed')).toHaveLength(3)
    expect(screen.queryByText('Waiting for response')).toBeNull()
  })

  it('renders an empty state when the remote session project group list is empty', async () => {
    await openRelayConsoleWithSessionPayload({ list: [], page: 1, page_size: 20, total: 0 })

    expect(await screen.findByText('No remote sessions to display')).not.toBeNull()
  })

  it('falls back to provider session status when runtime status is null', async () => {
    await openRelayConsoleWithSessionPayload({
      list: [
        {
          tool: 'codex',
          project_name: 'repo',
          project_path: '/repo',
          sessions: [
            {
              normalized_session_id: 'session-2',
              primary_session_id: 's2',
              title: 'Historical session',
              runtime_status: null,
              status: 'active',
              first_user_message_preview: 'Review old work',
              latest_event_summary: null,
              subagent_count: 0
            }
          ]
        }
      ],
      page: 1,
      page_size: 20,
      total: 1
    })

    expect(await screen.findByText('Historical session')).not.toBeNull()
    expect(screen.getByText('active')).not.toBeNull()
  })

  it('renders a session read error when the remote session RPC fails', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClients: Array<{ onStatus: (status: ConnectionStatus) => void }> = []
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      const client = {
        socket: {} as WebSocket,
        send: vi.fn(),
        close: vi.fn(),
        onStatus: options.onStatus,
        onMessage: options.onMessage
      }
      signalClients.push(client)
      return client
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    act(() => {
      signalClients[0]?.onStatus('accepted')
    })
    await waitFor(() => expect(createRelay).toHaveBeenCalledTimes(1))

    await act(async () => {
      openReadyRelay(relayOptions)
      relayOptions?.onPayload({
        version: 1,
        type: 'response',
        id: 'rpc_3',
        ok: false,
        error: { code: 'REMOTE_ERROR', message: 'failed' }
      })
      await Promise.resolve()
    })

    expect(await screen.findByText('Unable to read remote sessions')).not.toBeNull()
  })

  it('renders remote RPC error details in result cards', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClients: Array<{ onStatus: (status: ConnectionStatus) => void }> = []
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      const client = {
        socket: {} as WebSocket,
        send: vi.fn(),
        close: vi.fn(),
        onStatus: options.onStatus,
        onMessage: options.onMessage
      }
      signalClients.push(client)
      return client
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    act(() => {
      signalClients[0]?.onStatus('accepted')
    })
    await waitFor(() => expect(createRelay).toHaveBeenCalledTimes(1))

    await act(async () => {
      openReadyRelay(relayOptions)
      relayOptions?.onPayload({
        version: 1,
        type: 'response',
        id: 'rpc_1',
        ok: false,
        error: { code: 'method_not_found', message: 'unknown RPC method: rpc.ping' }
      })
      await Promise.resolve()
    })

    expect(screen.getByText('method_not_found: unknown RPC method: rpc.ping')).not.toBeNull()
  })

  it('closes pending RPC requests immediately when relay closes', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClients: Array<{ onStatus: (status: ConnectionStatus) => void }> = []
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      const client = {
        socket: {} as WebSocket,
        send: vi.fn(),
        close: vi.fn(),
        onStatus: options.onStatus,
        onMessage: options.onMessage
      }
      signalClients.push(client)
      return client
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    act(() => {
      signalClients[0]?.onStatus('accepted')
    })
    await waitFor(() => expect(createRelay).toHaveBeenCalledTimes(1))

    act(() => {
      openReadyRelay(relayOptions)
    })
    expect(screen.getAllByText('Waiting for response')).toHaveLength(4)

    await act(async () => {
      relayOptions?.onClose()
      await Promise.resolve()
    })

    expect(screen.getByText('Relay closed')).not.toBeNull()
    expect(screen.getAllByText('RPC request failed')).toHaveLength(3)
    expect(screen.queryByText('Waiting for response')).toBeNull()
  })

  it('closes relay socket when relay reports an error before close', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClients: Array<{ onStatus: (status: ConnectionStatus) => void }> = []
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      const client = {
        socket: {} as WebSocket,
        send: vi.fn(),
        close: vi.fn(),
        onStatus: options.onStatus,
        onMessage: options.onMessage
      }
      signalClients.push(client)
      return client
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    act(() => {
      signalClients[0]?.onStatus('accepted')
    })
    await waitFor(() => expect(createRelay).toHaveBeenCalledTimes(1))

    act(() => {
      openReadyRelay(relayOptions)
    })
    await act(async () => {
      relayOptions?.onError(new Error('invalid relay payload'))
      await Promise.resolve()
    })

    expect(relayClient.close).toHaveBeenCalledTimes(1)
    expect(screen.getByText('Relay error')).not.toBeNull()
    expect(screen.getAllByText('RPC request failed')).toHaveLength(3)
  })

  it('cleans relay RPC requests on unmount and ignores later relay callbacks', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const signalClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn(),
      onStatus: (_status: ConnectionStatus) => {},
      onMessage: (_value: unknown) => {}
    }
    const createConnection = vi.fn((options: ConnectionClientOptions) => {
      signalClient.onStatus = options.onStatus
      signalClient.onMessage = options.onMessage
      return signalClient
    })
    let relayOptions: RelayClientOptions | null = null
    const relayClient: RelayClient = {
      socket: {} as WebSocket,
      send: vi.fn(),
      close: vi.fn()
    }
    const createRelay = vi.fn((options: RelayClientOptions) => {
      relayOptions = options
      return relayClient
    })

    const { unmount } = render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        createRelay={createRelay}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))
    await waitFor(() => expect(createConnection).toHaveBeenCalledTimes(1))

    act(() => {
      signalClient.onStatus('accepted')
    })
    await waitFor(() => expect(createRelay).toHaveBeenCalledTimes(1))

    act(() => {
      openReadyRelay(relayOptions)
    })

    unmount()

    expect(signalClient.close).toHaveBeenCalledTimes(1)
    expect(relayClient.close).toHaveBeenCalledTimes(1)
    expect(() => {
      relayOptions?.onClose()
      relayOptions?.onError(new Error('late error'))
      relayOptions?.onPayload({ version: 1, type: 'response', id: 'rpc_1', ok: true, result: { pong: true } })
    }).not.toThrow()
  })
})
