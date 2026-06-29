import { afterEach, describe, expect, it, vi } from 'vitest'

import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteDevice } from '../api/devicesApi.js'
import type { ConnectionClientOptions, ConnectionStatus } from '../remote/connectionClient.js'
import {
  createRemoteDeviceSessionController,
  type RemoteDeviceSessionSnapshot
} from '../remote/remoteDeviceSessionController.js'
import type { RelayClient, RelayClientOptions } from '../remote/relayTransport.js'
import type { WebRtcTransport, WebRtcTransportOptions } from '../remote/webrtcTransport.js'

type MockFn<T extends (...args: never[]) => unknown> = ReturnType<typeof vi.fn<T>>

function createDevice(): RemoteDevice {
  return {
    id: 'device-1',
    name: 'Desk Mac',
    online: true,
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

type Harness = {
  snapshots: RemoteDeviceSessionSnapshot[]
  controller: ReturnType<typeof createRemoteDeviceSessionController>
  signalClient: {
    close: MockFn<() => void>
    send: MockFn<(value: unknown) => void>
    onStatus(status: ConnectionStatus): void
    onMessage(value: unknown): void
  }
  relayClient: RelayClient & { send: MockFn<(value: unknown) => void>; close: MockFn<() => void> }
  webRtcClient: WebRtcTransport & { send: MockFn<(value: unknown) => void>; close: MockFn<() => void> }
  getRelayOptions(): RelayClientOptions
  getWebRtcOptions(): WebRtcTransportOptions
}

function createHarness(
  controllerOptions: Partial<Parameters<typeof createRemoteDeviceSessionController>[0]> = {}
): Harness {
  const snapshots: RemoteDeviceSessionSnapshot[] = []
  let signalOptions: ConnectionClientOptions | null = null
  let relayOptions: RelayClientOptions | null = null
  let webRtcOptions: WebRtcTransportOptions | null = null

  const signalClient = {
    socket: {} as WebSocket,
    send: vi.fn<(value: unknown) => void>(),
    close: vi.fn<() => void>(),
    onStatus(status: ConnectionStatus) {
      signalOptions?.onStatus(status)
    },
    onMessage(value: unknown) {
      signalOptions?.onMessage(value)
    }
  }
  const relayClient = {
    socket: {} as WebSocket,
    send: vi.fn<(value: unknown) => void>(),
    close: vi.fn<() => void>()
  } satisfies RelayClient & { send: MockFn<(value: unknown) => void>; close: MockFn<() => void> }
  const webRtcClient = {
    kind: 'webrtc',
    start: vi.fn(async () => {}),
    acceptAnswer: vi.fn(async () => {}),
    addRemoteIceCandidate: vi.fn(async () => {}),
    send: vi.fn<(value: unknown) => void>(),
    close: vi.fn<() => void>()
  } satisfies WebRtcTransport & { send: MockFn<(value: unknown) => void>; close: MockFn<() => void> }

  const controller = createRemoteDeviceSessionController({
    device: createDevice(),
    connectionsApi: { create: vi.fn().mockResolvedValue(createConnectionResult()) },
    clientId: 'client-1',
    createConnection: (options) => {
      signalOptions = options
      return signalClient
    },
    createRelay: (options) => {
      relayOptions = options
      return relayClient
    },
    createWebRtc: (options) => {
      webRtcOptions = options
      return webRtcClient
    },
    onSnapshot: (snapshot) => {
      snapshots.push(snapshot)
    },
    ...controllerOptions
  })

  return {
    snapshots,
    controller,
    signalClient,
    relayClient,
    webRtcClient,
    getRelayOptions() {
      if (!relayOptions) throw new Error('relay options missing')
      return relayOptions
    },
    getWebRtcOptions() {
      if (!webRtcOptions) throw new Error('webrtc options missing')
      return webRtcOptions
    }
  }
}

async function connectAndAccept(harness: Harness) {
  await harness.controller.connect()
  harness.signalClient.onStatus('accepted')
  await Promise.resolve()
}

function latest(harness: Harness): RemoteDeviceSessionSnapshot {
  const snapshot = harness.snapshots.at(-1)
  if (!snapshot) throw new Error('snapshot missing')
  return snapshot
}

function openRelay(harness: Harness) {
  const relayOptions = harness.getRelayOptions()
  relayOptions.onOpen()
  relayOptions.onReady()
}

function respondRelay(harness: Harness, method: string, result: unknown) {
  const request = harness.relayClient.send.mock.calls
    .map((call) => call[0] as { id?: string; method?: string })
    .find((item) => item.method === method)
  if (!request?.id) throw new Error(`request missing: ${method}`)
  harness.getRelayOptions().onPayload({
    version: 1,
    type: 'response',
    id: request.id,
    ok: true,
    result
  })
}

afterEach(() => {
  vi.useRealTimers()
  vi.restoreAllMocks()
})

describe('createRemoteDeviceSessionController', () => {
  it('marks relay open after relay ready and starts the session stream', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)

    openRelay(harness)

    const snapshot = latest(harness)
    expect(snapshot.relayStatus).toBe('open')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.sessionsResult.status).toBe('loading')
    expect(harness.relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({
        method: 'local_api.stream',
        params: expect.objectContaining({
          path: '/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20'
        }),
        transport: { kind: 'relay' }
      })
    )
  })

  it('marks relay diagnostics ready only after the relay RPC ping responds', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)

    openRelay(harness)
    expect(latest(harness).diagnostics.relay.status).toBe('loading')

    respondRelay(harness, 'rpc.ping', { pong: true })

    expect(latest(harness).diagnostics.relay).toEqual({
      status: 'ready',
      value: { pong: true }
    })
  })

  it('promotes WebRTC after DataChannel open only when the WebRTC RPC ping responds', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)
    openRelay(harness)

    harness.getWebRtcOptions().onOpen()
    const probeRequest = harness.webRtcClient.send.mock.calls[0]?.[0] as { id?: string }
    expect(latest(harness).activeTransport).toBe('relay')

    harness.getWebRtcOptions().onPayload({
      version: 1,
      type: 'response',
      id: probeRequest.id,
      ok: true,
      result: { pong: true },
      transport: { kind: 'webrtc' }
    })

    const snapshot = latest(harness)
    expect(snapshot.webRtcStatus).toBe('open')
    expect(snapshot.activeTransport).toBe('webrtc')
    expect(snapshot.diagnostics.webrtc).toEqual({
      status: 'ready',
      value: { pong: true }
    })
  })

  it('falls back to relay when WebRTC business ping fails without blocking session reads', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)
    openRelay(harness)

    harness.getWebRtcOptions().onOpen()
    const probeRequest = harness.webRtcClient.send.mock.calls[0]?.[0] as { id?: string }
    harness.getWebRtcOptions().onPayload({
      version: 1,
      type: 'response',
      id: probeRequest.id,
      ok: false,
      error: { code: 'method_not_found', message: 'unknown RPC method: rpc.ping' },
      transport: { kind: 'webrtc' }
    })

    const snapshot = latest(harness)
    expect(snapshot.webRtcStatus).toBe('error')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.sessionsResult.status).toBe('loading')
    expect(harness.webRtcClient.close).toHaveBeenCalledTimes(1)
    expect(harness.relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({ method: 'local_api.stream', transport: { kind: 'relay' } })
    )
  })

  it('falls back to relay when WebRTC business ping times out', async () => {
    vi.useFakeTimers()
    const harness = createHarness({ rpcTimeoutMs: 30_000, webRtcProbeTimeoutMs: 1_000 })
    await connectAndAccept(harness)
    openRelay(harness)

    harness.getWebRtcOptions().onOpen()
    expect(latest(harness).diagnostics.webrtc.status).toBe('loading')

    await vi.advanceTimersByTimeAsync(1_000)

    const snapshot = latest(harness)
    expect(snapshot.webRtcStatus).toBe('error')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.diagnostics.webrtc.status).toBe('error')
    expect(snapshot.sessionsResult.status).toBe('loading')
  })

  it('ignores late callbacks after close', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)
    openRelay(harness)
    const countBeforeClose = harness.snapshots.length

    harness.controller.close()
    harness.getRelayOptions().onPayload({
      version: 1,
      type: 'response',
      id: 'rpc_1',
      ok: true,
      result: { pong: true }
    })
    harness.getRelayOptions().onClose()
    harness.getWebRtcOptions().onOpen()

    expect(harness.snapshots).toHaveLength(countBeforeClose)
    expect(harness.signalClient.close).toHaveBeenCalledTimes(1)
    expect(harness.relayClient.close).toHaveBeenCalledTimes(1)
    expect(harness.webRtcClient.close).toHaveBeenCalledTimes(1)
  })
})
