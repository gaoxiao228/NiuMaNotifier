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
  connectionsApi: { create: MockFn<(deviceId: string, clientId: string) => Promise<ConnectionCreateResult>> }
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
  const connectionsApi = {
    create: vi
      .fn<(deviceId: string, clientId: string) => Promise<ConnectionCreateResult>>()
      .mockResolvedValue(createConnectionResult())
  }

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
    connectionsApi,
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
    connectionsApi,
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

function latest(harness: { snapshots: RemoteDeviceSessionSnapshot[] }) {
  const snapshot = harness.snapshots[harness.snapshots.length - 1]
  if (!snapshot) throw new Error('expected at least one controller snapshot')
  return snapshot
}

async function openAcceptedConnection(harness: Harness) {
  await Promise.resolve()
  await Promise.resolve()
  harness.signalClient.onStatus('accepted')
  await Promise.resolve()
}

function openReadyRelay(relayOptions: RelayClientOptions) {
  relayOptions.onOpen()
  relayOptions.onReady()
}

function openWebRtc(harness: Harness) {
  harness.getWebRtcOptions().onOpen()
}

function respondWebRtcError(harness: Harness, method: string, error: { code: string; message: string }) {
  const request = harness.webRtcClient.send.mock.calls
    .map((call) => call[0] as { id?: string; method?: string })
    .find((item) => item.method === method)
  if (!request?.id) throw new Error(`request missing: ${method}`)
  harness.getWebRtcOptions().onPayload({
    version: 1,
    type: 'response',
    id: request.id,
    ok: false,
    error,
    transport: { kind: 'webrtc' }
  })
}

async function emitSessionGroup(harness: Harness, value: unknown) {
  respondRelay(harness, 'local_api.stream', { stream_id: 'stream_1' })
  await Promise.resolve()
  harness.getRelayOptions().onPayload({
    version: 1,
    type: 'notification',
    method: 'local_api.stream.event',
    params: {
      stream_id: 'stream_1',
      event: 'session_project_groups',
      id: '1',
      data: value
    }
  })
  await Promise.resolve()
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

function respondLatestRelay(harness: Harness, method: string, result: unknown) {
  const request = [...harness.relayClient.send.mock.calls]
    .reverse()
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

function countRelayRequests(harness: Harness, method: string): number {
  return harness.relayClient.send.mock.calls
    .map((call) => call[0] as { method?: string })
    .filter((item) => item.method === method).length
}

async function openRelayAndEmitSessionGroups(harness: Harness, value: unknown) {
  openRelay(harness)
  respondRelay(harness, 'local_api.stream', { stream_id: 'stream_1' })
  await Promise.resolve()
  harness.getRelayOptions().onPayload({
    version: 1,
    type: 'notification',
    method: 'local_api.stream.event',
    params: {
      stream_id: 'stream_1',
      event: 'session_project_groups',
      id: '1',
      data: value
    }
  })
  await Promise.resolve()
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

  it('creates a connection when diagnostics run without an active session', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 100 })

    await harness.controller.runDiagnostics()

    expect(harness.connectionsApi.create).toHaveBeenCalledTimes(1)
    expect(latest(harness).diagnosticReport?.steps.some((step) => step.key === 'connection_create')).toBe(true)
  })

  it('keeps diagnostics running while creating a connection and ignores duplicate runs', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 100 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()

    expect(latest(harness).diagnosticRunning).toBe(true)
    await harness.controller.runDiagnostics()
    expect(harness.connectionsApi.create).toHaveBeenCalledTimes(1)

    await diagnosticPromise
  })

  it('creates a new connection for diagnostics after the controller closes existing resources', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 100 })
    await connectAndAccept(harness)
    expect(harness.connectionsApi.create).toHaveBeenCalledTimes(1)

    harness.controller.close()
    await harness.controller.runDiagnostics()

    expect(harness.connectionsApi.create).toHaveBeenCalledTimes(2)
  })

  it('creates a new diagnostics connection when accepted signaling has lost remote resources', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 100 })
    await connectAndAccept(harness)
    openRelay(harness)
    expect(harness.connectionsApi.create).toHaveBeenCalledTimes(1)

    harness.getRelayOptions().onClose()
    expect(latest(harness).connectionStatus).toBe('accepted')

    await harness.controller.runDiagnostics()

    expect(harness.connectionsApi.create).toHaveBeenCalledTimes(2)
  })

  it('does not let stale diagnostics write into a newer connection generation', async () => {
    vi.useFakeTimers()
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 100 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()
    harness.controller.close()
    await harness.controller.connect()
    harness.signalClient.onStatus('accepted')
    await Promise.resolve()

    await vi.advanceTimersByTimeAsync(100)
    await diagnosticPromise

    expect(latest(harness).diagnosticReport).toBeNull()
    expect(latest(harness).diagnosticRunning).toBe(false)
  })

  it('emits diagnostics as stopped when closed during a diagnostics run', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 100 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()
    expect(latest(harness).diagnosticRunning).toBe(true)

    harness.controller.close()
    await diagnosticPromise

    expect(latest(harness).connectionStatus).toBe('idle')
    expect(latest(harness).diagnosticRunning).toBe(false)
  })

  it('finishes diagnostics when connection creation hangs', async () => {
    vi.useFakeTimers()
    const harness = createHarness({
      device: { ...createDevice(), online: true },
      rpcTimeoutMs: 100,
      connectionsApi: { create: vi.fn<() => Promise<ConnectionCreateResult>>(() => new Promise(() => {})) }
    })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()
    await vi.advanceTimersByTimeAsync(150)
    await diagnosticPromise

    const connectionStep = latest(harness).diagnosticReport?.steps.find((step) => step.key === 'connection_create')
    expect(latest(harness).diagnosticRunning).toBe(false)
    expect(connectionStep?.status).toBe('failed')
  })

  it('reruns relay diagnostics and session reads for an existing accepted connection', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true } })
    await connectAndAccept(harness)
    openRelay(harness)
    respondRelay(harness, 'rpc.ping', { pong: true })
    await emitSessionGroup(harness, { list: [{ project_name: 'old' }], page: 1, page_size: 20, total: 1 })
    const pingCountBefore = countRelayRequests(harness, 'rpc.ping')
    const streamCountBefore = countRelayRequests(harness, 'local_api.stream')

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()

    expect(countRelayRequests(harness, 'rpc.ping')).toBeGreaterThan(pingCountBefore)
    expect(countRelayRequests(harness, 'local_api.stream')).toBeGreaterThan(streamCountBefore)

    respondLatestRelay(harness, 'rpc.ping', { pong: true })
    respondLatestRelay(harness, 'local_api.stream', { stream_id: 'stream_2' })
    await Promise.resolve()
    harness.getRelayOptions().onPayload({
      version: 1,
      type: 'notification',
      method: 'local_api.stream.event',
      params: {
        stream_id: 'stream_2',
        event: 'session_project_groups',
        id: '2',
        data: { list: [], page: 1, page_size: 20, total: 0 }
      }
    })
    await Promise.resolve()
    await diagnosticPromise
  })

  it('fails pending relay diagnostics immediately when relay closes', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 1_000 })
    await connectAndAccept(harness)
    openRelay(harness)
    respondRelay(harness, 'rpc.ping', { pong: true })
    await emitSessionGroup(harness, { list: [], page: 1, page_size: 20, total: 0 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()
    harness.getRelayOptions().onClose()
    await diagnosticPromise

    const relayStep = latest(harness).diagnosticReport?.steps.find((step) => step.key === 'relay_rpc_ping')
    expect(relayStep).toMatchObject({
      status: 'failed',
      message: 'relay_closed'
    })
  })

  it('fails pending WebRTC diagnostics when relay close tears down remote resources', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 1_000 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await openAcceptedConnection(harness)
    openReadyRelay(harness.getRelayOptions())
    respondRelay(harness, 'rpc.ping', { pong: true })
    openWebRtc(harness)
    harness.getRelayOptions().onClose()
    expect(latest(harness).diagnostics.webrtc).toEqual({
      status: 'error',
      value: 'transport_closed'
    })
    await diagnosticPromise

    const webRtcStep = latest(harness).diagnosticReport?.steps.find((step) => step.key === 'webrtc_rpc_ping')
    expect(webRtcStep).toMatchObject({
      status: 'failed',
      message: 'transport_closed'
    })
  })

  it('waits longer than 250ms for diagnostics connection acceptance', async () => {
    vi.useFakeTimers()
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 1_000 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()
    await vi.advanceTimersByTimeAsync(300)

    expect(latest(harness).diagnosticRunning).toBe(true)

    await openAcceptedConnection(harness)
    await vi.advanceTimersByTimeAsync(50)
    openReadyRelay(harness.getRelayOptions())
    respondRelay(harness, 'rpc.ping', { pong: true })
    await vi.advanceTimersByTimeAsync(50)
    openWebRtc(harness)
    respondWebRtcError(harness, 'rpc.ping', { code: 'timeout', message: 'WebRTC ping timeout' })
    await vi.advanceTimersByTimeAsync(50)
    await emitSessionGroup(harness, { list: [], page: 1, page_size: 20, total: 0 })
    await vi.advanceTimersByTimeAsync(50)

    await diagnosticPromise
  })

  it('fails diagnostics immediately when signaling rejects the connection', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 1_000 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()
    await Promise.resolve()
    harness.signalClient.onStatus('rejected')
    await diagnosticPromise

    const connectionStep = latest(harness).diagnosticReport?.steps.find((step) => step.key === 'connection_create')
    expect(connectionStep).toMatchObject({
      status: 'failed',
      message: 'connection_rejected'
    })
    expect(latest(harness).diagnosticRunning).toBe(false)
  })

  it('fails diagnostics immediately when signaling closes the connection', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true }, rpcTimeoutMs: 1_000 })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await Promise.resolve()
    await Promise.resolve()
    harness.signalClient.onStatus('closed')
    await diagnosticPromise

    const report = latest(harness).diagnosticReport
    const connectionStep = report?.steps.find((step) => step.key === 'connection_create')
    expect(connectionStep).toMatchObject({
      status: 'failed',
      message: 'connection_closed'
    })
    expect(report?.steps.filter((step) => step.status === 'skipped').map((step) => step.key)).toEqual([
      'relay_rpc_ping',
      'webrtc_rpc_ping',
      'session_project_groups'
    ])
    expect(latest(harness).diagnosticRunning).toBe(false)
  })

  it('reports degraded when relay ping works and WebRTC ping fails', async () => {
    const harness = createHarness({ device: { ...createDevice(), online: true } })

    const diagnosticPromise = harness.controller.runDiagnostics()
    await openAcceptedConnection(harness)
    openReadyRelay(harness.getRelayOptions())
    respondRelay(harness, 'rpc.ping', { pong: true })
    openWebRtc(harness)
    respondWebRtcError(harness, 'rpc.ping', { code: 'timeout', message: 'WebRTC ping timeout' })
    await emitSessionGroup(harness, { list: [], page: 1, page_size: 20, total: 0 })

    await diagnosticPromise

    expect(latest(harness).diagnosticReport?.overall).toBe('degraded')
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

  it('keeps relay resources alive when WebRTC ping fails before relay is ready', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)

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

    openRelay(harness)

    const snapshot = latest(harness)
    expect(snapshot.relayStatus).toBe('open')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.sessionsResult.status).toBe('loading')
    expect(harness.relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({ method: 'local_api.stream', transport: { kind: 'relay' } })
    )
  })

  it('keeps relay resources alive when WebRTC ping times out before relay is ready', async () => {
    vi.useFakeTimers()
    const harness = createHarness({ rpcTimeoutMs: 30_000, webRtcProbeTimeoutMs: 1_000 })
    await connectAndAccept(harness)

    harness.getWebRtcOptions().onOpen()
    await vi.advanceTimersByTimeAsync(1_000)

    openRelay(harness)

    const snapshot = latest(harness)
    expect(snapshot.relayStatus).toBe('open')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.sessionsResult.status).toBe('loading')
    expect(harness.relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({ method: 'local_api.stream', transport: { kind: 'relay' } })
    )
  })

  it('keeps relay resources alive when WebRTC errors before relay is ready', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)

    harness.getWebRtcOptions().onError(new Error('webrtc failed'))
    openRelay(harness)

    const snapshot = latest(harness)
    expect(snapshot.webRtcStatus).toBe('error')
    expect(snapshot.relayStatus).toBe('open')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.sessionsResult.status).toBe('loading')
    expect(harness.relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({ method: 'local_api.stream', transport: { kind: 'relay' } })
    )
  })

  it('keeps relay resources alive when WebRTC closes before relay is ready', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)

    harness.getWebRtcOptions().onClose()
    openRelay(harness)

    const snapshot = latest(harness)
    expect(snapshot.webRtcStatus).toBe('closed')
    expect(snapshot.relayStatus).toBe('open')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.sessionsResult.status).toBe('loading')
    expect(harness.relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({ method: 'local_api.stream', transport: { kind: 'relay' } })
    )
  })

  it('keeps relay resources alive when WebRTC start rejects before relay is ready', async () => {
    let webRtcOptions: WebRtcTransportOptions | null = null
    const harness = createHarness({
      createWebRtc: (options) => {
        webRtcOptions = options
        return {
          kind: 'webrtc',
          start: vi.fn(async () => {
            throw new Error('start failed')
          }),
          acceptAnswer: vi.fn(async () => {}),
          addRemoteIceCandidate: vi.fn(async () => {}),
          send: vi.fn<(value: unknown) => void>(),
          close: vi.fn<() => void>()
        }
      }
    })
    await connectAndAccept(harness)
    expect(webRtcOptions).not.toBeNull()
    await Promise.resolve()

    openRelay(harness)

    const snapshot = latest(harness)
    expect(snapshot.webRtcStatus).toBe('error')
    expect(snapshot.relayStatus).toBe('open')
    expect(snapshot.activeTransport).toBe('relay')
    expect(snapshot.sessionsResult.status).toBe('loading')
    expect(harness.relayClient.send).toHaveBeenCalledWith(
      expect.objectContaining({ method: 'local_api.stream', transport: { kind: 'relay' } })
    )
  })

  it('protects controller snapshots from consumer mutation', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)
    await openRelayAndEmitSessionGroups(harness, {
      list: [{ project_name: 'repo' }],
      page: 1,
      page_size: 20,
      total: 1
    })

    const emitted = latest(harness).sessionsResult.value as { list: Array<{ project_name: string }> }
    emitted.list[0].project_name = 'mutated'
    respondRelay(harness, 'state.get', { state: 'ready' })
    await Promise.resolve()

    const nextValue = latest(harness).sessionsResult.value as { list: Array<{ project_name: string }> }
    expect(nextValue.list[0].project_name).toBe('repo')
  })

  it('ignores late callbacks after close', async () => {
    const harness = createHarness()
    await connectAndAccept(harness)
    openRelay(harness)
    const countBeforeClose = harness.snapshots.length

    harness.controller.close()
    const countAfterClose = harness.snapshots.length
    harness.getRelayOptions().onPayload({
      version: 1,
      type: 'response',
      id: 'rpc_1',
      ok: true,
      result: { pong: true }
    })
    harness.getRelayOptions().onClose()
    harness.getWebRtcOptions().onOpen()

    expect(countAfterClose).toBe(countBeforeClose + 1)
    expect(harness.snapshots).toHaveLength(countAfterClose)
    expect(latest(harness).connectionStatus).toBe('idle')
    expect(harness.signalClient.close).toHaveBeenCalledTimes(1)
    expect(harness.relayClient.close).toHaveBeenCalledTimes(1)
    expect(harness.webRtcClient.close).toHaveBeenCalledTimes(1)
  })
})
