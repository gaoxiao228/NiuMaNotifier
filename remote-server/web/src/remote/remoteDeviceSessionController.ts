import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteDevice } from '../api/devicesApi.js'
import {
  buildClientSocketUrl,
  createConnectionClient,
  type ConnectionClient,
  type ConnectionClientOptions,
  type ConnectionStatus
} from './connectionClient.js'
import {
  createPlainRpcClient,
  createPlainRpcRequest,
  isPlainRpcResponse,
  PlainRpcTimeoutError,
  type PlainRpcClient,
  type PlainRpcResponse,
  type RemoteTransportKind
} from './plainRpcClient.js'
import {
  buildRelaySocketUrl,
  createRelayClient,
  type RelayClient,
  type RelayClientOptions
} from './relayTransport.js'
import { createRemoteLocalApiClient } from './remoteLocalApiClient.js'
import { createRemoteMessageBus, type RemoteMessageBus } from './remoteTransport.js'
import {
  createWebRtcTransport,
  type WebRtcAnswerSignal,
  type WebRtcIceCandidateSignal,
  type WebRtcTransport,
  type WebRtcTransportOptions
} from './webrtcTransport.js'

export type RpcResultState = {
  status: 'idle' | 'loading' | 'ready' | 'error'
  value: unknown
}

export type RemoteDeviceTransportStatus = 'idle' | 'connecting' | 'open' | 'closed' | 'error'
export type RemoteDeviceActiveTransport = 'idle' | 'relay' | 'webrtc'

export type RemoteDeviceSessionSnapshot = {
  connectionStatus: ConnectionStatus | 'idle'
  relayStatus: RemoteDeviceTransportStatus
  webRtcStatus: RemoteDeviceTransportStatus
  activeTransport: RemoteDeviceActiveTransport
  connectionId: string | null
  error: string | null
  pingResult: RpcResultState
  stateResult: RpcResultState
  sessionsResult: RpcResultState
  diagnostics: {
    relay: RpcResultState
    webrtc: RpcResultState
  }
}

export type RemoteDeviceSessionController = {
  connect(): Promise<void>
  close(): void
  handleSignalMessage(message: unknown): void
}

type ConnectionsApi = {
  create(deviceId: string, clientId: string): Promise<ConnectionCreateResult>
}

type StreamHandle = {
  close(): Promise<void>
}

type PendingDiagnostic = {
  id: string
  timer: ReturnType<typeof setTimeout>
  onReady?(value: unknown): void
  onError?(error: Error): void
}

type RemoteDeviceSessionControllerOptions = {
  device: RemoteDevice
  connectionsApi: ConnectionsApi
  clientId: string
  createConnection?: (options: ConnectionClientOptions) => ConnectionClient
  createRelay?: (options: RelayClientOptions) => RelayClient
  createWebRtc?: (options: WebRtcTransportOptions) => WebRtcTransport
  onSnapshot(snapshot: RemoteDeviceSessionSnapshot): void
  onSignalMessage?(message: unknown): void
  rpcTimeoutMs?: number
  webRtcProbeTimeoutMs?: number
}

const SESSION_GROUPS_STREAM_PATH = '/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20'

function idleResult(): RpcResultState {
  return { status: 'idle', value: null }
}

function loadingResult(): RpcResultState {
  return { status: 'loading', value: null }
}

function readyResult(value: unknown): RpcResultState {
  return { status: 'ready', value }
}

function errorResult(value: unknown): RpcResultState {
  return { status: 'error', value }
}

function initialSnapshot(): RemoteDeviceSessionSnapshot {
  return {
    connectionStatus: 'idle',
    relayStatus: 'idle',
    webRtcStatus: 'idle',
    activeTransport: 'idle',
    connectionId: null,
    error: null,
    pingResult: idleResult(),
    stateResult: idleResult(),
    sessionsResult: idleResult(),
    diagnostics: {
      relay: idleResult(),
      webrtc: idleResult()
    }
  }
}

function cloneResult(result: RpcResultState): RpcResultState {
  return { status: result.status, value: result.value }
}

function cloneSnapshot(snapshot: RemoteDeviceSessionSnapshot): RemoteDeviceSessionSnapshot {
  return {
    ...snapshot,
    pingResult: cloneResult(snapshot.pingResult),
    stateResult: cloneResult(snapshot.stateResult),
    sessionsResult: cloneResult(snapshot.sessionsResult),
    diagnostics: {
      relay: cloneResult(snapshot.diagnostics.relay),
      webrtc: cloneResult(snapshot.diagnostics.webrtc)
    }
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function signalMessageType(value: unknown): string | null {
  return isRecord(value) && typeof value.type === 'string' ? value.type : null
}

function signalMessageData(value: unknown): Record<string, unknown> | null {
  if (!isRecord(value) || !isRecord(value.data)) return null
  return value.data
}

function readWebRtcAnswerSignal(value: unknown, connectionId: string): WebRtcAnswerSignal | null {
  const data = signalMessageData(value)
  if (signalMessageType(value) !== 'signal.answer' || data?.connection_id !== connectionId || typeof data.sdp !== 'string') {
    return null
  }
  return { connection_id: connectionId, sdp: data.sdp }
}

function readWebRtcIceCandidateSignal(value: unknown, connectionId: string): WebRtcIceCandidateSignal | null {
  const data = signalMessageData(value)
  if (
    signalMessageType(value) === 'signal.ice_candidate' &&
    data?.connection_id === connectionId &&
    typeof data.candidate === 'string' &&
    (typeof data.sdp_mid === 'undefined' || data.sdp_mid === null || typeof data.sdp_mid === 'string') &&
    (typeof data.sdp_mline_index === 'undefined' ||
      data.sdp_mline_index === null ||
      typeof data.sdp_mline_index === 'number')
  ) {
    return {
      connection_id: connectionId,
      candidate: data.candidate,
      sdp_mid: data.sdp_mid as string | null | undefined,
      sdp_mline_index: data.sdp_mline_index as number | null | undefined
    }
  }
  return null
}

function errorText(error: unknown, fallback: string): string {
  return error instanceof Error && error.message ? error.message : fallback
}

function responseErrorText(response: PlainRpcResponse): string {
  if (!isRecord(response.error)) return 'remote_rpc_failed'
  const code = typeof response.error.code === 'string' ? response.error.code.trim() : ''
  const message = typeof response.error.message === 'string' ? response.error.message.trim() : ''
  if (code && message) return `${code}: ${message}`
  return message || code || 'remote_rpc_failed'
}

export function createRemoteDeviceSessionController(
  options: RemoteDeviceSessionControllerOptions
): RemoteDeviceSessionController {
  const createConnection = options.createConnection ?? createConnectionClient
  const createRelay = options.createRelay ?? createRelayClient
  const createWebRtc = options.createWebRtc ?? createWebRtcTransport
  const rpcTimeoutMs = options.rpcTimeoutMs ?? 10_000
  const webRtcProbeTimeoutMs = options.webRtcProbeTimeoutMs ?? rpcTimeoutMs

  let snapshot = initialSnapshot()
  let activeGeneration = 0
  let socketClient: ConnectionClient | null = null
  let relayClient: RelayClient | null = null
  let webRtcTransport: WebRtcTransport | null = null
  let messageBus: RemoteMessageBus | null = null
  let rpcClient: PlainRpcClient | null = null
  let remoteLocalApi: ReturnType<typeof createRemoteLocalApiClient> | null = null
  let sessionStream: StreamHandle | null = null
  let relayOpen = false
  let webRtcOpen = false
  let remoteReadsStarted = false
  let retryRemoteReadsOnRelayReady = false
  let nextSignalSeq = 1
  let nextDiagnosticSeq = 1
  const diagnostics = new Map<RemoteTransportKind, PendingDiagnostic>()

  function isActive(generation: number): boolean {
    return activeGeneration === generation
  }

  function emit(generation: number) {
    if (!isActive(generation)) return
    options.onSnapshot(cloneSnapshot(snapshot))
  }

  function patchSnapshot(generation: number, patch: Partial<RemoteDeviceSessionSnapshot>) {
    if (!isActive(generation)) return
    snapshot = { ...snapshot, ...patch }
    emit(generation)
  }

  function patchResult(
    generation: number,
    key: 'pingResult' | 'stateResult' | 'sessionsResult',
    value: RpcResultState
  ) {
    patchSnapshot(generation, { [key]: value } as Partial<RemoteDeviceSessionSnapshot>)
  }

  function patchDiagnostic(generation: number, kind: RemoteTransportKind, value: RpcResultState) {
    if (!isActive(generation)) return
    snapshot = {
      ...snapshot,
      diagnostics: {
        ...snapshot.diagnostics,
        [kind]: value
      }
    }
    emit(generation)
  }

  function clearDiagnostic(kind: RemoteTransportKind) {
    const pending = diagnostics.get(kind)
    if (!pending) return
    clearTimeout(pending.timer)
    diagnostics.delete(kind)
  }

  function closeRemoteResources() {
    for (const kind of diagnostics.keys()) clearDiagnostic(kind)
    void sessionStream?.close().catch(() => {})
    sessionStream = null
    remoteLocalApi = null
    rpcClient?.close()
    rpcClient = null
    messageBus?.close()
    messageBus = null
    webRtcTransport?.close()
    webRtcTransport = null
    relayClient?.close()
    relayClient = null
    relayOpen = false
    webRtcOpen = false
    remoteReadsStarted = false
    retryRemoteReadsOnRelayReady = false
  }

  function closeAllResources() {
    socketClient?.close()
    socketClient = null
    closeRemoteResources()
  }

  function nextSignalId(prefix: string): string {
    const seq = nextSignalSeq
    nextSignalSeq += 1
    return `msg_${prefix}_${seq}`
  }

  function handleWebRtcSignal(value: unknown, connectionId: string) {
    const transport = webRtcTransport
    if (!transport) return

    const answer = readWebRtcAnswerSignal(value, connectionId)
    if (answer) {
      void transport.acceptAnswer(answer).catch(() => {})
      return
    }

    const candidate = readWebRtcIceCandidateSignal(value, connectionId)
    if (candidate) {
      void transport.addRemoteIceCandidate(candidate).catch(() => {})
    }
  }

  function handleSignalMessage(message: unknown) {
    const connectionId = snapshot.connectionId
    if (!connectionId) return
    handleWebRtcSignal(message, connectionId)
  }

  function markWebRtcUnhealthyAndUseRelay(generation: number) {
    clearDiagnostic('webrtc')
    webRtcOpen = false
    messageBus?.setOpen('webrtc', false)
    patchSnapshot(generation, {
      webRtcStatus: 'error',
      activeTransport: relayOpen ? 'relay' : 'idle'
    })
    webRtcTransport?.close()
  }

  function requestWithTransportFallback(generation: number, method: string, params?: unknown): Promise<unknown> {
    if (!rpcClient) return Promise.reject(new Error('remote_rpc_unavailable'))

    const request = rpcClient.request(method, params)
    if (!webRtcOpen) return request

    return request.catch((error) => {
      if (
        error instanceof PlainRpcTimeoutError &&
        error.transportKind === 'webrtc' &&
        relayOpen &&
        isActive(generation)
      ) {
        markWebRtcUnhealthyAndUseRelay(generation)
        return rpcClient?.request(method, params) ?? Promise.reject(new Error('remote_rpc_unavailable'))
      }
      if (
        error instanceof PlainRpcTimeoutError &&
        error.transportKind === 'webrtc' &&
        !relayOpen &&
        isActive(generation)
      ) {
        retryRemoteReadsOnRelayReady = true
        markWebRtcUnhealthyAndUseRelay(generation)
      }
      throw error
    })
  }

  function updateRpcResult(
    generation: number,
    method: string,
    key: 'pingResult' | 'stateResult',
    params?: unknown
  ) {
    patchResult(generation, key, loadingResult())
    void requestWithTransportFallback(generation, method, params)
      .then((value) => {
        patchResult(generation, key, readyResult(value))
      })
      .catch((error) => {
        patchResult(generation, key, errorResult(errorText(error, 'remote_rpc_failed')))
      })
  }

  function subscribeRemoteSessions(generation: number) {
    if (!remoteLocalApi) return
    patchResult(generation, 'sessionsResult', loadingResult())
    void remoteLocalApi
      .stream(SESSION_GROUPS_STREAM_PATH, {
        onEvent(event) {
          if (event.event !== 'session_project_groups') return
          patchResult(generation, 'sessionsResult', readyResult(event.data))
        },
        onClosed() {
          patchResult(generation, 'sessionsResult', errorResult('remote_rpc_failed'))
        },
        onError(error) {
          patchResult(generation, 'sessionsResult', errorResult(errorText(error, 'remote_rpc_failed')))
        }
      })
      .then((stream) => {
        if (isActive(generation)) {
          sessionStream = stream
        } else {
          void stream.close().catch(() => {})
        }
      })
      .catch((error) => {
        patchResult(generation, 'sessionsResult', errorResult(errorText(error, 'remote_rpc_failed')))
      })
  }

  function startRemoteReadsOnce(generation: number) {
    if (remoteReadsStarted) return
    remoteReadsStarted = true
    updateRpcResult(generation, 'rpc.ping', 'pingResult')
    updateRpcResult(generation, 'state.get', 'stateResult')
    subscribeRemoteSessions(generation)
  }

  function retryRemoteReadsThroughRelay(generation: number) {
    retryRemoteReadsOnRelayReady = false
    updateRpcResult(generation, 'rpc.ping', 'pingResult')
    updateRpcResult(generation, 'state.get', 'stateResult')
    subscribeRemoteSessions(generation)
  }

  function finishDiagnostic(
    generation: number,
    kind: RemoteTransportKind,
    status: 'ready' | 'error',
    value: unknown
  ) {
    const pending = diagnostics.get(kind)
    if (!pending) return
    clearDiagnostic(kind)
    if (!isActive(generation)) return

    patchDiagnostic(generation, kind, status === 'ready' ? readyResult(value) : errorResult(value))
    if (status === 'ready') {
      pending.onReady?.(value)
    } else {
      pending.onError?.(value instanceof Error ? value : new Error(String(value)))
    }
  }

  function handleDiagnosticPayload(generation: number, payload: unknown): boolean {
    if (!isPlainRpcResponse(payload)) return false

    for (const [kind, pending] of diagnostics) {
      if (pending.id !== payload.id) continue
      if (payload.ok) {
        finishDiagnostic(generation, kind, 'ready', payload.result)
      } else {
        finishDiagnostic(generation, kind, 'error', responseErrorText(payload))
      }
      return true
    }
    return false
  }

  function startDiagnosticPing(
    generation: number,
    kind: RemoteTransportKind,
    timeoutMs: number,
    callbacks: Pick<PendingDiagnostic, 'onReady' | 'onError'> = {}
  ) {
    const bus = messageBus
    if (!bus) return

    clearDiagnostic(kind)
    patchDiagnostic(generation, kind, loadingResult())

    const id = `diag_${kind}_${nextDiagnosticSeq}`
    nextDiagnosticSeq += 1
    const timer = setTimeout(() => {
      finishDiagnostic(generation, kind, 'error', new PlainRpcTimeoutError(kind))
    }, timeoutMs)
    diagnostics.set(kind, { id, timer, ...callbacks })

    try {
      // 诊断 ping 必须走指定通道，不能只根据 socket/DataChannel open 推断业务可用。
      bus.sendVia(kind, createPlainRpcRequest(id, 'rpc.ping', {}, kind))
    } catch (error) {
      finishDiagnostic(generation, kind, 'error', error instanceof Error ? error : new Error('remote_rpc_failed'))
    }
  }

  function openRemoteSession(result: ConnectionCreateResult, generation: number, signalClient: ConnectionClient) {
    closeRemoteResources()
    patchSnapshot(generation, {
      relayStatus: 'connecting',
      webRtcStatus: 'connecting',
      activeTransport: 'idle',
      pingResult: idleResult(),
      stateResult: idleResult(),
      sessionsResult: idleResult(),
      diagnostics: {
        relay: idleResult(),
        webrtc: idleResult()
      }
    })

    const relayUrl = buildRelaySocketUrl(result.relay_url || result.signaling_url || window.location.origin, {
      connection_id: result.connection_id,
      connection_token: result.connection_token,
      side: 'client'
    })

    messageBus = createRemoteMessageBus({
      onInbound: (message) => {
        if (!isActive(generation)) return
        handleDiagnosticPayload(generation, message.payload)
        rpcClient?.handle(message)
      }
    })
    rpcClient = createPlainRpcClient({
      timeoutMs: rpcTimeoutMs,
      send: (value) => messageBus?.send(value),
      onNotification: ({ method, params, observedTransport, declaredTransport }) => {
        remoteLocalApi?.handleNotification(method, params, {
          observedTransport,
          declaredTransport
        })
      }
    })
    remoteLocalApi = createRemoteLocalApiClient({
      request: (method, params) => requestWithTransportFallback(generation, method, params)
    })

    if (createWebRtc !== createWebRtcTransport || typeof RTCPeerConnection !== 'undefined') {
      const transport = createWebRtc({
        connectionId: result.connection_id,
        onOffer: (offer) => {
          signalClient.send({
            version: 1,
            type: 'signal.offer',
            id: nextSignalId('offer'),
            data: {
              sdp: offer.sdp
            }
          })
        },
        onIceCandidate: (candidate) => {
          signalClient.send({
            version: 1,
            type: 'signal.ice_candidate',
            id: nextSignalId('ice'),
            data: {
              candidate: candidate.candidate,
              sdp_mid: candidate.sdp_mid ?? null,
              sdp_mline_index: candidate.sdp_mline_index ?? null
            }
          })
        },
        onOpen: () => {
          if (!isActive(generation) || !messageBus) return
          webRtcOpen = false
          messageBus.setOpen('webrtc', true)
          patchSnapshot(generation, { webRtcStatus: 'connecting' })
          startDiagnosticPing(generation, 'webrtc', webRtcProbeTimeoutMs, {
            onReady: () => {
              if (!isActive(generation) || !messageBus) return
              webRtcOpen = true
              messageBus.setOpen('webrtc', true)
              patchSnapshot(generation, {
                webRtcStatus: 'open',
                activeTransport: 'webrtc'
              })
              startRemoteReadsOnce(generation)
            },
            onError: () => {
              if (!isActive(generation) || !messageBus) return
              messageBus.setOpen('webrtc', false)
              markWebRtcUnhealthyAndUseRelay(generation)
              if (!relayOpen) closeRemoteResources()
            }
          })
          if (!webRtcOpen) messageBus?.setOpen('webrtc', false)
        },
        onPayload: (value) => {
          if (!isActive(generation) || !messageBus) return
          if (webRtcOpen || diagnostics.has('webrtc')) messageBus.receive(value, 'webrtc')
        },
        onClose: () => {
          if (!isActive(generation)) return
          clearDiagnostic('webrtc')
          webRtcOpen = false
          messageBus?.setOpen('webrtc', false)
          patchSnapshot(generation, {
            webRtcStatus: 'closed',
            activeTransport: relayOpen ? 'relay' : 'idle'
          })
          if (!relayOpen) closeRemoteResources()
        },
        onError: () => {
          if (!isActive(generation)) return
          clearDiagnostic('webrtc')
          webRtcOpen = false
          messageBus?.setOpen('webrtc', false)
          patchSnapshot(generation, {
            webRtcStatus: 'error',
            activeTransport: relayOpen ? 'relay' : 'idle'
          })
          if (!relayOpen) closeRemoteResources()
        }
      })
      webRtcTransport = transport
      messageBus.register(transport)
      void transport.start().catch(() => {
        if (!isActive(generation)) return
        webRtcOpen = false
        messageBus?.setOpen('webrtc', false)
        patchSnapshot(generation, {
          webRtcStatus: 'error',
          activeTransport: relayOpen ? 'relay' : 'idle'
        })
        if (!relayOpen) closeRemoteResources()
      })
    }

    relayClient = createRelay({
      url: relayUrl,
      connectionId: result.connection_id,
      onOpen: () => {
        patchSnapshot(generation, { relayStatus: 'connecting' })
      },
      onReady: () => {
        if (!isActive(generation) || !messageBus || !relayClient) return
        messageBus.register({
          kind: 'relay',
          send: (value) => relayClient?.send(value),
          close: () => relayClient?.close()
        })
        messageBus.setOpen('relay', true)
        relayOpen = true
        patchSnapshot(generation, {
          relayStatus: 'open',
          activeTransport: webRtcOpen ? 'webrtc' : 'relay'
        })
        startDiagnosticPing(generation, 'relay', rpcTimeoutMs)
        if (retryRemoteReadsOnRelayReady) {
          retryRemoteReadsThroughRelay(generation)
          return
        }
        startRemoteReadsOnce(generation)
      },
      onPayload: (value) => {
        if (isActive(generation)) messageBus?.receive(value, 'relay')
      },
      onClose: () => {
        if (!isActive(generation)) return
        clearDiagnostic('relay')
        relayOpen = false
        messageBus?.setOpen('relay', false)
        if (webRtcOpen) {
          patchSnapshot(generation, { relayStatus: 'closed' })
          return
        }
        closeRemoteResources()
        patchSnapshot(generation, {
          relayStatus: 'closed',
          webRtcStatus: 'closed',
          activeTransport: 'idle'
        })
      },
      onError: () => {
        if (!isActive(generation)) return
        clearDiagnostic('relay')
        relayOpen = false
        messageBus?.setOpen('relay', false)
        if (webRtcOpen) {
          relayClient?.close()
          patchSnapshot(generation, { relayStatus: 'error' })
          return
        }
        closeRemoteResources()
        patchSnapshot(generation, {
          relayStatus: 'error',
          webRtcStatus: 'closed',
          activeTransport: 'idle'
        })
      }
    })
  }

  return {
    async connect() {
      if (!options.device.online || snapshot.connectionStatus === 'connecting') return
      const generation = activeGeneration + 1
      activeGeneration = generation
      closeAllResources()
      snapshot = {
        ...initialSnapshot(),
        connectionStatus: 'connecting'
      }
      emit(generation)

      try {
        const result = await options.connectionsApi.create(options.device.id, options.clientId)
        if (!isActive(generation)) return
        patchSnapshot(generation, {
          connectionId: result.connection_id
        })
        const socketUrl = buildClientSocketUrl(result.signaling_url || window.location.origin, {
          connection_id: result.connection_id,
          connection_token: result.connection_token
        })
        let relayStarted = false
        let signalClient: ConnectionClient | null = null
        signalClient = createConnection({
          url: socketUrl,
          onStatus: (status) => {
            if (!isActive(generation)) return
            patchSnapshot(generation, { connectionStatus: status })
            if (status === 'accepted' && !relayStarted && signalClient) {
              relayStarted = true
              openRemoteSession(result, generation, signalClient)
            }
          },
          onMessage: (message) => {
            if (!isActive(generation)) return
            options.onSignalMessage?.(message)
            handleWebRtcSignal(message, result.connection_id)
          }
        })
        socketClient = signalClient
      } catch (error) {
        if (!isActive(generation)) return
        patchSnapshot(generation, {
          connectionStatus: 'error',
          error: errorText(error, 'connection_failed')
        })
      }
    },
    close() {
      activeGeneration += 1
      closeAllResources()
    },
    handleSignalMessage
  }
}
