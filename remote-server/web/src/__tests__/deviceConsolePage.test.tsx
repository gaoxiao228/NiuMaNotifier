import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { RemoteDevice } from '../api/devicesApi.js'
import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import { DeviceConsolePage } from '../remote/deviceConsolePage.js'
import type { ConnectionClientOptions, ConnectionStatus } from '../remote/connectionClient.js'
import type { RelayClient, RelayClientOptions } from '../remote/relayTransport.js'
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

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('DeviceConsolePage', () => {
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

    expect(relayClient.send).toHaveBeenCalledTimes(3)
    expect(relayClient.send).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({ id: 'rpc_1', method: 'rpc.ping' })
    )
    expect(relayClient.send).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({ id: 'rpc_2', method: 'state.get' })
    )
    expect(relayClient.send).toHaveBeenNthCalledWith(
      3,
      expect.objectContaining({ id: 'rpc_3', method: 'session.list' })
    )

    act(() => {
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
        result: { sessions: [{ id: 's1' }] }
      })
    })

    expect(await screen.findByText('Ping')).not.toBeNull()
    expect(screen.getByText(/"pong": true/)).not.toBeNull()
    expect(screen.getByText(/"state": "ready"/)).not.toBeNull()
    expect(screen.getByText(/"id": "s1"/)).not.toBeNull()
  })
})
