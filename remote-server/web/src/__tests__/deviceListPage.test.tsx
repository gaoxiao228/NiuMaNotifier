import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteDevice } from '../api/devicesApi.js'
import { DeviceListPage } from '../devices/deviceListPage.js'
import { createTranslator } from '../i18n/index.js'
import type { ConnectionClientOptions } from '../remote/connectionClient.js'
import {
  createRemoteDeviceSessionController,
  type RemoteDeviceSessionSnapshot
} from '../remote/remoteDeviceSessionController.js'
import type { RelayClient, RelayClientOptions } from '../remote/relayTransport.js'
import type { WebRtcTransport, WebRtcTransportOptions } from '../remote/webrtcTransport.js'

type Deferred<T> = {
  promise: Promise<T>
  resolve(value: T): void
}

function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

const t = createTranslator('en')

function createDevice(id: string, name: string, online = true): RemoteDevice {
  return {
    id,
    name,
    online,
    last_seen_at: null,
    capabilities: {},
    identity_public_key: {}
  }
}

function createConnectionResult(): ConnectionCreateResult {
  return {
    connection_id: 'conn_123',
    connection_token: 'short_token',
    expires_at: '2026-06-29T00:00:00Z',
    expires_in: 60,
    signaling_url: 'ws://127.0.0.1:27880/ws/client',
    relay_url: 'https://relay.example.com'
  }
}

function createSnapshot(
  patch: Partial<RemoteDeviceSessionSnapshot> = {}
): RemoteDeviceSessionSnapshot {
  return {
    connectionStatus: 'accepted',
    relayStatus: 'idle',
    webRtcStatus: 'idle',
    activeTransport: 'idle',
    connectionId: 'conn_123',
    error: null,
    pingResult: { status: 'idle', value: null },
    stateResult: { status: 'idle', value: null },
    sessionsResult: { status: 'idle', value: null },
    diagnostics: {
      relay: { status: 'idle', value: null },
      webrtc: { status: 'idle', value: null }
    },
    ...patch
  }
}

async function renderDeviceListWithDevices(
  list: RemoteDevice[],
  props: Partial<ComponentProps<typeof DeviceListPage>> = {}
) {
  const devicesApi = {
    list: vi.fn().mockResolvedValue({ list })
  }
  const connectionsApi = {
    create: vi.fn().mockResolvedValue(createConnectionResult())
  }
  const result = render(
    <DeviceListPage
      devicesApi={devicesApi}
      connectionsApi={connectionsApi}
      clientId="client-test"
      t={t}
      onSelectDevice={() => {}}
      {...props}
    />
  )
  await screen.findByText('Devices')
  return { ...result, devicesApi, connectionsApi }
}

function findRpcRequestId(calls: unknown[][], method: string, prefix?: string): string {
  const request = calls
    .map((call) => call[0] as { id?: string; method?: string })
    .find((item) => item.method === method && (!prefix || item.id?.startsWith(prefix)))
  if (!request?.id) throw new Error(`request missing: ${method}`)
  return request.id
}

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('DeviceListPage', () => {
  it('keeps the latest refresh result when an older request returns later', async () => {
    const first = createDeferred<{ list: RemoteDevice[] }>()
    const second = createDeferred<{ list: RemoteDevice[] }>()
    const firstApi = {
      list: () => first.promise
    }
    const secondApi = {
      list: () => second.promise
    }

    const createRemoteSessionController = vi.fn(() => ({
      connect: vi.fn(async () => {}),
      close: vi.fn(),
      handleSignalMessage: vi.fn()
    }))
    const { rerender } = render(
      <DeviceListPage
        devicesApi={firstApi}
        connectionsApi={{ create: vi.fn() }}
        t={t}
        onSelectDevice={() => {}}
        createRemoteSessionController={createRemoteSessionController}
      />
    )
    rerender(
      <DeviceListPage
        devicesApi={secondApi}
        connectionsApi={{ create: vi.fn() }}
        t={t}
        onSelectDevice={() => {}}
        createRemoteSessionController={createRemoteSessionController}
      />
    )

    await act(async () => {
      second.resolve({
        list: [
          {
            id: 'dev_new',
            name: 'New device',
            online: true,
            last_seen_at: null,
            capabilities: {},
            identity_public_key: {}
          }
        ]
      })
      await second.promise
    })

    await screen.findAllByText('New device')

    await act(async () => {
      first.resolve({
        list: [
          {
            id: 'dev_old',
            name: 'Old device',
            online: true,
            last_seen_at: null,
            capabilities: {},
            identity_public_key: {}
          }
        ]
      })
      await first.promise
    })

    expect(screen.getAllByText('New device').length).toBeGreaterThan(0)
    expect(screen.queryByText('Old device')).toBeNull()
  })

  it('automatically connects the first online device after loading devices', async () => {
    const connect = vi.fn(async () => {})
    const createRemoteSessionController = vi.fn((options: Parameters<typeof createRemoteDeviceSessionController>[0]) => {
      // 显式接收 options 让 mock.calls 保留 controller 工厂参数类型。
      void options
      return {
        connect,
        close: vi.fn(),
        handleSignalMessage: vi.fn()
      }
    })

    await renderDeviceListWithDevices(
      [createDevice('offline-1', 'Offline laptop', false), createDevice('online-1', 'Desk Mac')],
      { createRemoteSessionController }
    )

    await waitFor(() => expect(createRemoteSessionController).toHaveBeenCalledTimes(1))
    expect(createRemoteSessionController.mock.calls[0]?.[0].device.id).toBe('online-1')
    expect(connect).toHaveBeenCalledTimes(1)
    expect(screen.getAllByText('Desk Mac').length).toBeGreaterThan(0)
  })

  it('starts the remote session stream after relay is ready from the list page connection', async () => {
    let signalOptions: ConnectionClientOptions | null = null
    let relayOptions: RelayClientOptions | null = null
    const relayClient = {
      socket: {} as WebSocket,
      send: vi.fn<(value: unknown) => void>(),
      close: vi.fn<() => void>()
    } satisfies RelayClient
    const webRtcClient = {
      kind: 'webrtc',
      start: vi.fn(async () => {}),
      acceptAnswer: vi.fn(async () => {}),
      addRemoteIceCandidate: vi.fn(async () => {}),
      send: vi.fn<(value: unknown) => void>(),
      close: vi.fn<() => void>()
    } satisfies WebRtcTransport
    const createRemoteSessionController = (options: Parameters<typeof createRemoteDeviceSessionController>[0]) =>
      createRemoteDeviceSessionController({
        ...options,
        createConnection: (nextOptions) => {
          signalOptions = nextOptions
          return {
            socket: {} as WebSocket,
            send: vi.fn(),
            close: vi.fn()
          }
        },
        createRelay: (nextOptions) => {
          relayOptions = nextOptions
          return relayClient
        },
        createWebRtc: vi.fn((options: WebRtcTransportOptions) => {
          void options
          return webRtcClient
        })
      })

    await renderDeviceListWithDevices([createDevice('device-1', 'Desk Mac')], {
      createRemoteSessionController
    })
    await waitFor(() => expect(signalOptions).not.toBeNull())

    await act(async () => {
      signalOptions?.onStatus('accepted')
      await Promise.resolve()
    })
    await waitFor(() => expect(relayOptions).not.toBeNull())

    await act(async () => {
      relayOptions?.onOpen()
      relayOptions?.onReady()
      await Promise.resolve()
    })

    expect(screen.getByText('Relay open')).not.toBeNull()
    expect(screen.getByText('Waiting for response')).not.toBeNull()
    expect(relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'local_api.stream',
        params: expect.objectContaining({
          path: '/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20'
        }),
        transport: { kind: 'relay' }
      })
    )
  })

  it('renders a session received from the remote session stream', async () => {
    let signalOptions: ConnectionClientOptions | null = null
    let relayOptions: RelayClientOptions | null = null
    const relayClient = {
      socket: {} as WebSocket,
      send: vi.fn<(value: unknown) => void>(),
      close: vi.fn<() => void>()
    } satisfies RelayClient
    const createRemoteSessionController = (options: Parameters<typeof createRemoteDeviceSessionController>[0]) =>
      createRemoteDeviceSessionController({
        ...options,
        createConnection: (nextOptions) => {
          signalOptions = nextOptions
          return { socket: {} as WebSocket, send: vi.fn(), close: vi.fn() }
        },
        createRelay: (nextOptions) => {
          relayOptions = nextOptions
          return relayClient
        },
        createWebRtc: () => ({
          kind: 'webrtc',
          start: vi.fn(async () => {}),
          acceptAnswer: vi.fn(async () => {}),
          addRemoteIceCandidate: vi.fn(async () => {}),
          send: vi.fn<(value: unknown) => void>(),
          close: vi.fn<() => void>()
        })
      })

    await renderDeviceListWithDevices([createDevice('device-1', 'Desk Mac')], {
      createRemoteSessionController
    })
    await waitFor(() => expect(signalOptions).not.toBeNull())

    await act(async () => {
      signalOptions?.onStatus('accepted')
      await Promise.resolve()
      relayOptions?.onReady()
      await Promise.resolve()
    })

    const streamRequestId = findRpcRequestId(relayClient.send.mock.calls, 'local_api.stream')
    await act(async () => {
      relayOptions?.onPayload({
        version: 1,
        type: 'response',
        id: streamRequestId,
        ok: true,
        result: { stream_id: 'stream_1' }
      })
      await Promise.resolve()
      relayOptions?.onPayload({
        version: 1,
        type: 'notification',
        method: 'local_api.stream.event',
        params: {
          stream_id: 'stream_1',
          event: 'session_project_groups',
          id: 'event_1',
          data: {
            list: [
              {
                tool: 'codex',
                project_name: 'NiuMaNotifier',
                project_path: '/Users/niuma/code/NiuMaNotifier',
                sessions: [
                  {
                    normalized_session_id: 'session-1',
                    title: 'Review remote relay',
                    runtime_status: 'running',
                    first_user_message_preview: 'Open the device list'
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

    expect(screen.getByText('NiuMaNotifier')).not.toBeNull()
    expect(screen.getByText('Review remote relay')).not.toBeNull()
    expect(screen.getByText('running')).not.toBeNull()
  })

  it('shows relay and WebRTC diagnostic states from the controller snapshot', async () => {
    let pushSnapshot!: (snapshot: RemoteDeviceSessionSnapshot) => void
    const createRemoteSessionController = vi.fn((options: Parameters<typeof createRemoteDeviceSessionController>[0]) => {
      pushSnapshot = options.onSnapshot
      return {
        connect: vi.fn(async () => {
          pushSnapshot(
            createSnapshot({
              relayStatus: 'open',
              webRtcStatus: 'open',
              activeTransport: 'webrtc',
              diagnostics: {
                relay: { status: 'ready', value: { pong: true } },
                webrtc: { status: 'ready', value: { pong: true } }
              }
            })
          )
        }),
        close: vi.fn(),
        handleSignalMessage: vi.fn()
      }
    })

    await renderDeviceListWithDevices([createDevice('device-1', 'Desk Mac')], {
      createRemoteSessionController
    })

    const rpcStatus = await screen.findByLabelText('Remote RPC status')
    await waitFor(() => expect(within(rpcStatus).getByText('Relay open')).not.toBeNull())
    expect(within(rpcStatus).getByText('WebRTC open')).not.toBeNull()
    expect(within(rpcStatus).getByText('Active transport: WebRTC')).not.toBeNull()
    expect(within(rpcStatus).getByText('Relay diagnostics')).not.toBeNull()
    expect(within(rpcStatus).getByText('WebRTC diagnostics')).not.toBeNull()
    expect(within(rpcStatus).getAllByText('Ready')).toHaveLength(2)
  })

  it('does not create a remote session controller when no devices are online', async () => {
    const createRemoteSessionController = vi.fn()

    await renderDeviceListWithDevices([createDevice('device-1', 'Desk Mac', false)], {
      createRemoteSessionController
    })

    await screen.findByText('No online devices')
    expect(createRemoteSessionController).not.toHaveBeenCalled()
  })

  it('closes the old controller when the first online device changes and on unmount', async () => {
    const firstClose = vi.fn()
    const secondClose = vi.fn()
    const createRemoteSessionController = vi
      .fn()
      .mockReturnValueOnce({ connect: vi.fn(async () => {}), close: firstClose, handleSignalMessage: vi.fn() })
      .mockReturnValueOnce({ connect: vi.fn(async () => {}), close: secondClose, handleSignalMessage: vi.fn() })
    const firstApi = {
      list: vi.fn().mockResolvedValue({ list: [createDevice('device-1', 'Desk Mac')] })
    }
    const secondApi = {
      list: vi.fn().mockResolvedValue({ list: [createDevice('device-2', 'Studio Mac')] })
    }
    const connectionsApi = {
      create: vi.fn()
    }

    const { rerender, unmount } = render(
      <DeviceListPage
        devicesApi={firstApi}
        connectionsApi={connectionsApi}
        clientId="client-test"
        t={t}
        onSelectDevice={() => {}}
        createRemoteSessionController={createRemoteSessionController}
      />
    )
    await waitFor(() => expect(createRemoteSessionController).toHaveBeenCalledTimes(1))

    rerender(
      <DeviceListPage
        devicesApi={secondApi}
        connectionsApi={connectionsApi}
        clientId="client-test"
        t={t}
        onSelectDevice={() => {}}
        createRemoteSessionController={createRemoteSessionController}
      />
    )
    await waitFor(() => expect(createRemoteSessionController).toHaveBeenCalledTimes(2))
    expect(firstClose).toHaveBeenCalledTimes(1)

    unmount()
    expect(secondClose).toHaveBeenCalledTimes(1)
  })
})
