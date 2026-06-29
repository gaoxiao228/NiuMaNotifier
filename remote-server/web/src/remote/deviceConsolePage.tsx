import { ArrowLeft, PlugZap, TerminalSquare } from 'lucide-react'
import type { Dispatch, SetStateAction } from 'react'
import { useEffect, useMemo, useRef, useState } from 'react'

import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteDevice } from '../api/devicesApi.js'
import { toDisplayErrorMessage } from '../shared/errorMessage.js'
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
  type PlainRpcClient
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

type ConnectionsApi = {
  create(deviceId: string, clientId: string): Promise<ConnectionCreateResult>
}

type RpcResultState = {
  status: 'idle' | 'loading' | 'ready' | 'error'
  value: unknown
}

type ActiveTransportState = 'idle' | 'relay' | 'webrtc'
type TransportConnectionState = 'idle' | 'connecting' | 'open' | 'closed' | 'error'

type RemoteSessionProjectGroupPage = {
  list: RemoteSessionProjectGroup[]
  page?: number
  page_size?: number
  total?: number
}

type RemoteSessionProjectGroup = {
  tool?: string
  project_name?: string
  project_path?: string
  sessions: RemoteSessionSummary[]
}

type RemoteSessionSummary = {
  normalized_session_id?: string
  primary_session_id?: string
  title?: string
  status?: string
  runtime_status?: string | null
  updated_at?: string
  first_user_message_preview?: string
  latest_event_summary?: string | null
  subagent_count?: number
}

type DeviceConsolePageProps = {
  device: RemoteDevice
  connectionsApi: ConnectionsApi
  createConnection?: (options: ConnectionClientOptions) => ConnectionClient
  createRelay?: (options: RelayClientOptions) => RelayClient
  createWebRtc?: (options: WebRtcTransportOptions) => WebRtcTransport
  autoConnect?: boolean
  t: (key: string) => string
  onBack(): void
}

const CLIENT_ID_KEY = 'niuma.remote.client_id'
let memoryClientId: string | null = null

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

function isRemoteSessionSummary(value: unknown): value is RemoteSessionSummary {
  return (
    isRecord(value) &&
    (typeof value.normalized_session_id === 'undefined' || typeof value.normalized_session_id === 'string') &&
    (typeof value.primary_session_id === 'undefined' || typeof value.primary_session_id === 'string') &&
    (typeof value.title === 'undefined' || typeof value.title === 'string') &&
    (typeof value.status === 'undefined' || typeof value.status === 'string') &&
    (typeof value.runtime_status === 'undefined' ||
      value.runtime_status === null ||
      typeof value.runtime_status === 'string') &&
    (typeof value.updated_at === 'undefined' || typeof value.updated_at === 'string') &&
    (typeof value.first_user_message_preview === 'undefined' || typeof value.first_user_message_preview === 'string') &&
    (typeof value.latest_event_summary === 'undefined' ||
      value.latest_event_summary === null ||
      typeof value.latest_event_summary === 'string') &&
    (typeof value.subagent_count === 'undefined' || typeof value.subagent_count === 'number')
  )
}

function isProjectGroupPage(value: unknown): value is RemoteSessionProjectGroupPage {
  // RPC result 是 unknown；这里只收窄列表渲染必须依赖的字段。
  return (
    isRecord(value) &&
    Array.isArray(value.list) &&
    value.list.every(
      (group) =>
        isRecord(group) &&
        (typeof group.tool === 'undefined' || typeof group.tool === 'string') &&
        (typeof group.project_name === 'undefined' || typeof group.project_name === 'string') &&
        (typeof group.project_path === 'undefined' || typeof group.project_path === 'string') &&
        Array.isArray(group.sessions) &&
        group.sessions.every(isRemoteSessionSummary)
    )
  )
}

function sessionDisplayStatus(session: RemoteSessionSummary): string | null {
  return session.runtime_status || session.status || null
}

function sessionTitle(session: RemoteSessionSummary): string {
  return session.title || session.primary_session_id || session.normalized_session_id || ''
}

function sessionDescription(session: RemoteSessionSummary): string | null {
  return session.first_user_message_preview || session.latest_event_summary || session.primary_session_id || null
}

function randomClientId(): string {
  const randomPart =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : Math.random().toString(36).slice(2)
  return `niuma-web-client-${randomPart}`
}

function resolveClientStorage(): Storage | null {
  if (typeof window === 'undefined') return null

  try {
    const descriptor = Object.getOwnPropertyDescriptor(window, 'localStorage')
    if (descriptor && 'value' in descriptor) return descriptor.value as Storage
    if (typeof process !== 'undefined' && process.versions?.node) return null
    if (window.location.protocol === 'about:') return null
    return window.localStorage
  } catch {
    return null
  }
}

function getStableClientId(): string {
  const fallback = memoryClientId ?? randomClientId()
  memoryClientId = fallback
  const storage = resolveClientStorage()
  if (!storage) return fallback

  try {
    const current = storage.getItem(CLIENT_ID_KEY)
    if (current) return current
    storage.setItem(CLIENT_ID_KEY, fallback)
  } catch {
    // 浏览器禁用 storage 时仍允许本次页面会话继续发起连接。
  }
  return fallback
}

function displaySignalMessage(value: unknown): string {
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
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

export function DeviceConsolePage({
  device,
  connectionsApi,
  createConnection = createConnectionClient,
  createRelay = createRelayClient,
  createWebRtc = createWebRtcTransport,
  autoConnect = false,
  t,
  onBack
}: DeviceConsolePageProps) {
  const clientId = useMemo(() => getStableClientId(), [])
  const [status, setStatus] = useState<ConnectionStatus | 'idle'>('idle')
  const [relayStatus, setRelayStatus] = useState<TransportConnectionState>('idle')
  const [webRtcStatus, setWebRtcStatus] = useState<TransportConnectionState>('idle')
  const [activeTransport, setActiveTransport] = useState<ActiveTransportState>('idle')
  const [connectionId, setConnectionId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [messages, setMessages] = useState<unknown[]>([])
  const [pingResult, setPingResult] = useState<RpcResultState>({ status: 'idle', value: null })
  const [stateResult, setStateResult] = useState<RpcResultState>({ status: 'idle', value: null })
  const [sessionsResult, setSessionsResult] = useState<RpcResultState>({ status: 'idle', value: null })
  const socketRef = useRef<ConnectionClient | null>(null)
  const relayRef = useRef<RelayClient | null>(null)
  const relayOpenRef = useRef(false)
  const webRtcRef = useRef<WebRtcTransport | null>(null)
  const webRtcOpenRef = useRef(false)
  const messageBusRef = useRef<RemoteMessageBus | null>(null)
  const rpcRef = useRef<PlainRpcClient | null>(null)
  const remoteLocalApiRef = useRef<ReturnType<typeof createRemoteLocalApiClient> | null>(null)
  const sessionStreamRef = useRef<{ close(): Promise<void> } | null>(null)
  const activeConnectionRef = useRef(0)
  const nextSignalSeqRef = useRef(1)
  const mountedRef = useRef(false)
  const autoConnectStartedRef = useRef(false)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      activeConnectionRef.current += 1
      socketRef.current?.close()
      void sessionStreamRef.current?.close().catch(() => {})
      rpcRef.current?.close()
      messageBusRef.current?.close()
      webRtcRef.current?.close()
      relayRef.current?.close()
      socketRef.current = null
      sessionStreamRef.current = null
      remoteLocalApiRef.current = null
      messageBusRef.current = null
      rpcRef.current = null
      webRtcRef.current = null
      relayOpenRef.current = false
      webRtcOpenRef.current = false
      relayRef.current = null
    }
  }, [])

  function isActiveConnection(connectionId: number): boolean {
    return mountedRef.current && activeConnectionRef.current === connectionId
  }

  function resetRelayConsole() {
    setRelayStatus('idle')
    setWebRtcStatus('idle')
    setActiveTransport('idle')
    setPingResult({ status: 'idle', value: null })
    setStateResult({ status: 'idle', value: null })
    setSessionsResult({ status: 'idle', value: null })
  }

  function closeRelayConsole() {
    void sessionStreamRef.current?.close().catch(() => {})
    rpcRef.current?.close()
    messageBusRef.current?.close()
    webRtcRef.current?.close()
    relayRef.current?.close()
    sessionStreamRef.current = null
    remoteLocalApiRef.current = null
    messageBusRef.current = null
    rpcRef.current = null
    webRtcRef.current = null
    relayOpenRef.current = false
    webRtcOpenRef.current = false
    relayRef.current = null
  }

  function nextSignalId(prefix: string): string {
    const seq = nextSignalSeqRef.current
    nextSignalSeqRef.current += 1
    return `msg_${prefix}_${seq}`
  }

  function handleWebRtcSignal(value: unknown, expectedConnectionId: string) {
    const transport = webRtcRef.current
    if (!transport) return

    const answer = readWebRtcAnswerSignal(value, expectedConnectionId)
    if (answer) {
      void transport.acceptAnswer(answer).catch(() => {})
      return
    }

    const candidate = readWebRtcIceCandidateSignal(value, expectedConnectionId)
    if (candidate) {
      void transport.addRemoteIceCandidate(candidate).catch(() => {})
    }
  }

  function displayJson(value: unknown): string {
    if (value === null) return t('waiting_for_relay')
    if (typeof value === 'string') return value
    try {
      return JSON.stringify(value, null, 2)
    } catch {
      return String(value)
    }
  }

  function displayRpcResult(result: RpcResultState): string {
    if (result.status === 'idle') return t('waiting_for_relay')
    if (result.status === 'loading') return t('waiting_for_response')
    return displayJson(result.value)
  }

  function rpcErrorText(error: unknown): string {
    // 本地清理 pending 请求属于连接生命周期收敛，不把内部实现文案直接暴露给用户。
    if (error instanceof Error && error.message === 'Plain RPC client closed') return t('remote_rpc_failed')
    return error instanceof Error && error.message ? error.message : t('remote_rpc_failed')
  }

  function activeTransportLabel(): string {
    return t(`active_transport_${activeTransport}`)
  }

  useEffect(() => {
    if (!autoConnect || autoConnectStartedRef.current || !device.online) return
    autoConnectStartedRef.current = true
    // 从设备列表点击“连接”进入控制台时，自动发起一次真实连接。
    void handleConnect()
  }, [autoConnect, device.online])

  function renderSessionGroups() {
    if (sessionsResult.status === 'idle') return <p className="state-message">{t('waiting_for_relay')}</p>
    if (sessionsResult.status === 'loading') return <p className="state-message">{t('waiting_for_response')}</p>
    if (sessionsResult.status === 'error' || !isProjectGroupPage(sessionsResult.value)) {
      return (
        <p className="state-message state-message-error" role="alert">
          {t('remote_sessions_failed')}
        </p>
      )
    }

    const groups = sessionsResult.value.list
    if (groups.length === 0 || groups.every((group) => group.sessions.length === 0)) {
      return <p className="state-message">{t('remote_sessions_empty')}</p>
    }

    return (
      <div className="remote-session-groups">
        {groups.map((group, groupIndex) => (
          <div className="remote-session-group" key={`${group.project_path ?? group.project_name ?? groupIndex}`}>
            <div className="remote-session-group-heading">
              {group.project_name ? <strong>{group.project_name}</strong> : null}
              {group.project_path ? <span>{group.project_path}</span> : null}
              {group.tool ? <span>{group.tool}</span> : null}
            </div>
            <div className="remote-session-list">
              {group.sessions.map((session, sessionIndex) => {
                const displayStatus = sessionDisplayStatus(session)
                const description = sessionDescription(session)
                return (
                  <div
                    className="remote-session-row"
                    key={session.normalized_session_id ?? session.primary_session_id ?? `${groupIndex}-${sessionIndex}`}
                  >
                    <div className="remote-session-main">
                      <strong>{sessionTitle(session)}</strong>
                      {description ? <span>{description}</span> : null}
                    </div>
                    {displayStatus ? <span className="remote-session-status">{displayStatus}</span> : null}
                  </div>
                )
              })}
            </div>
          </div>
        ))}
      </div>
    )
  }

  function openRelayConsole(
    result: ConnectionCreateResult,
    activeConnectionId: number,
    signalClient: ConnectionClient
  ) {
    closeRelayConsole()
    resetRelayConsole()
    setRelayStatus('connecting')
    setWebRtcStatus('connecting')

    const relayUrl = buildRelaySocketUrl(result.relay_url || result.signaling_url || window.location.origin, {
      connection_id: result.connection_id,
      connection_token: result.connection_token,
      side: 'client'
    })
    let relayClient: RelayClient
    const messageBus = createRemoteMessageBus({
      onInbound: (message) => {
        rpcClient.handle(message)
      }
    })
    const rpcClient = createPlainRpcClient({
      timeoutMs: 10_000,
      send: (value) => {
        messageBus.send(value)
      },
      onNotification: ({ method, params, observedTransport, declaredTransport }) => {
        remoteLocalApiRef.current?.handleNotification(method, params, {
          observedTransport,
          declaredTransport
        })
      }
    })
    messageBusRef.current = messageBus
    let webRtcProbeId: string | null = null

    const remoteLocalApi = createRemoteLocalApiClient({
      request: (method, params) => rpcClient.request(method, params)
    })
    remoteLocalApiRef.current = remoteLocalApi

    function closeRemoteRpcConsole(options: { closeRelay: boolean; closeWebRtc: boolean }) {
      // 所有传输通道都不可用后，本轮请求已不会再收到响应，立即关闭 RPC 以清理 pending。
      webRtcProbeId = null
      void sessionStreamRef.current?.close().catch(() => {})
      rpcClient.close()
      messageBus.close()
      if (options.closeWebRtc) webRtcRef.current?.close()
      if (options.closeRelay) relayClient.close()
      sessionStreamRef.current = null
      if (remoteLocalApiRef.current === remoteLocalApi) remoteLocalApiRef.current = null
      if (messageBusRef.current === messageBus) messageBusRef.current = null
      if (rpcRef.current === rpcClient) rpcRef.current = null
      if (webRtcRef.current) webRtcRef.current = null
      relayOpenRef.current = false
      webRtcOpenRef.current = false
      setWebRtcStatus('closed')
      setActiveTransport('idle')
      if (relayRef.current === relayClient) relayRef.current = null
    }

    if (createWebRtc !== createWebRtcTransport || typeof RTCPeerConnection !== 'undefined') {
      const webRtcTransport = createWebRtc({
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
          if (!isActiveConnection(activeConnectionId)) return
          webRtcOpenRef.current = false
          messageBus.setOpen('webrtc', false)
          setWebRtcStatus('connecting')
          webRtcProbeId = `rpc_webrtc_probe_${activeConnectionId}`
          try {
            // DataChannel open 只代表传输层可写；先用 plain RPC 探活，成功后才提升为主通道。
            webRtcTransport.send(createPlainRpcRequest(webRtcProbeId, 'rpc.ping', {}, 'webrtc'))
          } catch {
            webRtcProbeId = null
            setWebRtcStatus('error')
            setActiveTransport(relayOpenRef.current ? 'relay' : 'idle')
          }
        },
        onPayload: (value) => {
          if (!isActiveConnection(activeConnectionId)) return
          if (webRtcProbeId && isPlainRpcResponse(value) && value.id === webRtcProbeId) {
            webRtcProbeId = null
            if (value.ok) {
              webRtcOpenRef.current = true
              messageBus.setOpen('webrtc', true)
              setWebRtcStatus('open')
              setActiveTransport('webrtc')
              return
            }
            webRtcOpenRef.current = false
            messageBus.setOpen('webrtc', false)
            setWebRtcStatus('error')
            setActiveTransport(relayOpenRef.current ? 'relay' : 'idle')
            webRtcTransport.close()
            if (!relayOpenRef.current) closeRemoteRpcConsole({ closeRelay: false, closeWebRtc: false })
            return
          }
          if (webRtcOpenRef.current) messageBus.receive(value, 'webrtc')
        },
        onClose: () => {
          if (!isActiveConnection(activeConnectionId)) return
          webRtcProbeId = null
          webRtcOpenRef.current = false
          messageBus.setOpen('webrtc', false)
          setWebRtcStatus('closed')
          setActiveTransport(relayOpenRef.current ? 'relay' : 'idle')
          if (!relayOpenRef.current) closeRemoteRpcConsole({ closeRelay: false, closeWebRtc: false })
        },
        onError: () => {
          if (!isActiveConnection(activeConnectionId)) return
          webRtcProbeId = null
          webRtcOpenRef.current = false
          messageBus.setOpen('webrtc', false)
          setWebRtcStatus('error')
          setActiveTransport(relayOpenRef.current ? 'relay' : 'idle')
          if (!relayOpenRef.current) closeRemoteRpcConsole({ closeRelay: false, closeWebRtc: false })
        }
      })
      webRtcRef.current = webRtcTransport
      messageBus.register(webRtcTransport)
      // WebRTC 后台协商失败不影响 relay 首屏可用性。
      void webRtcTransport.start().catch(() => {
        if (isActiveConnection(activeConnectionId)) {
          webRtcProbeId = null
          webRtcOpenRef.current = false
          messageBus.setOpen('webrtc', false)
          setWebRtcStatus('error')
          setActiveTransport(relayOpenRef.current ? 'relay' : 'idle')
          if (!relayOpenRef.current) closeRemoteRpcConsole({ closeRelay: false, closeWebRtc: false })
        }
      })
    }

    function updateRpcResult(
      method: string,
      setResult: Dispatch<SetStateAction<RpcResultState>>,
      params?: unknown
    ) {
      setResult({ status: 'loading', value: null })
      void rpcClient
        .request(method, params)
        .then((value) => {
          if (isActiveConnection(activeConnectionId)) setResult({ status: 'ready', value })
        })
        .catch((error) => {
          if (isActiveConnection(activeConnectionId)) {
            setResult({ status: 'error', value: rpcErrorText(error) })
          }
        })
    }

    function subscribeRemoteSessions() {
      setSessionsResult({ status: 'loading', value: null })
      void remoteLocalApi
        .stream('/api/v1/session_project_groups/stream?tool=codex&page=1&page_size=20', {
          onEvent(event) {
            if (event.event !== 'session_project_groups') return
            if (isActiveConnection(activeConnectionId)) {
              setSessionsResult({ status: 'ready', value: event.data })
            }
          },
          onClosed() {
            if (isActiveConnection(activeConnectionId)) {
              setSessionsResult({ status: 'error', value: t('remote_rpc_failed') })
            }
          },
          onError() {
            if (isActiveConnection(activeConnectionId)) {
              setSessionsResult({ status: 'error', value: t('remote_rpc_failed') })
            }
          }
        })
        .then((stream) => {
          if (isActiveConnection(activeConnectionId)) {
            sessionStreamRef.current = stream
          } else {
            void stream.close().catch(() => {})
          }
        })
        .catch((error) => {
          if (isActiveConnection(activeConnectionId)) {
            setSessionsResult({ status: 'error', value: rpcErrorText(error) })
          }
        })
    }

    relayClient = createRelay({
      url: relayUrl,
      connectionId: result.connection_id,
      onOpen: () => {
        if (!isActiveConnection(activeConnectionId)) return
        setRelayStatus('connecting')
      },
      onReady: () => {
        if (!isActiveConnection(activeConnectionId)) return
        messageBus.register({
          kind: 'relay',
          send: (value) => relayClient.send(value),
          close: () => relayClient.close()
        })
        messageBus.setOpen('relay', true)
        relayOpenRef.current = true
        setRelayStatus('open')
        if (!webRtcOpenRef.current) setActiveTransport('relay')
        updateRpcResult('rpc.ping', setPingResult)
        updateRpcResult('state.get', setStateResult)
        subscribeRemoteSessions()
      },
      onPayload: (value) => {
        if (isActiveConnection(activeConnectionId)) {
          messageBus.receive(value, 'relay')
        }
      },
      onClose: () => {
        if (!isActiveConnection(activeConnectionId)) return
        messageBus.setOpen('relay', false)
        relayOpenRef.current = false
        if (webRtcOpenRef.current) {
          if (relayRef.current === relayClient) relayRef.current = null
          setRelayStatus('closed')
          return
        }
        closeRemoteRpcConsole({ closeRelay: false, closeWebRtc: true })
        setRelayStatus('closed')
      },
      onError: () => {
        if (!isActiveConnection(activeConnectionId)) return
        messageBus.setOpen('relay', false)
        relayOpenRef.current = false
        if (webRtcOpenRef.current) {
          relayClient.close()
          if (relayRef.current === relayClient) relayRef.current = null
          setRelayStatus('error')
          return
        }
        closeRemoteRpcConsole({ closeRelay: true, closeWebRtc: true })
        setRelayStatus('error')
      }
    })

    relayRef.current = relayClient
    rpcRef.current = rpcClient
  }

  async function handleConnect() {
    if (!device.online || status === 'connecting') return
    const activeConnectionId = activeConnectionRef.current + 1
    activeConnectionRef.current = activeConnectionId
    socketRef.current?.close()
    closeRelayConsole()
    socketRef.current = null

    setStatus('connecting')
    setError(null)
    setMessages([])
    resetRelayConsole()

    try {
      const result = await connectionsApi.create(device.id, clientId)
      if (!isActiveConnection(activeConnectionId)) return
      setConnectionId(result.connection_id)
      const socketUrl = buildClientSocketUrl(result.signaling_url || window.location.origin, {
        connection_id: result.connection_id,
        connection_token: result.connection_token
      })
      let relayStarted = false
      let signalClient: ConnectionClient | null = null
      signalClient = createConnection({
        url: socketUrl,
        onStatus: (nextStatus) => {
          if (!isActiveConnection(activeConnectionId)) return
          setStatus(nextStatus)
          if (nextStatus === 'accepted' && !relayStarted && signalClient) {
            relayStarted = true
            openRelayConsole(result, activeConnectionId, signalClient)
          }
        },
        onMessage: (value) => {
          if (isActiveConnection(activeConnectionId)) {
            setMessages((current) => [value, ...current].slice(0, 20))
            handleWebRtcSignal(value, result.connection_id)
          }
        }
      })
      socketRef.current = signalClient
    } catch (err) {
      if (!isActiveConnection(activeConnectionId)) return
      setStatus('error')
      setError(toDisplayErrorMessage(t, err, 'connection_failed'))
    }
  }

  return (
    <section className="device-panel console-panel">
      <div className="panel-title device-panel-heading">
        <span className="panel-title-label">
          <TerminalSquare aria-hidden="true" size={18} />
          <span>{t('device_console')}</span>
        </span>
        <button type="button" className="secondary-button" onClick={onBack}>
          <ArrowLeft aria-hidden="true" size={15} />
          {t('back_to_devices')}
        </button>
      </div>

      <div className="console-summary" aria-label={t('device_console')}>
        <div>
          <span className="summary-label">{t('identifier')}</span>
          <strong>{device.name || device.id}</strong>
        </div>
        <div>
          <span className="summary-label">{t('state')}</span>
          <strong className={`status-${device.online ? 'online' : 'offline'}`}>
            {t(device.online ? 'online' : 'offline')}
          </strong>
        </div>
        <div>
          <span className="summary-label">{t('connection_status')}</span>
          <strong>{t(`connection_status_${status}`)}</strong>
        </div>
        {connectionId ? (
          <div>
            <span className="summary-label">{t('connection_id')}</span>
            <strong className="device-id">{connectionId}</strong>
          </div>
        ) : null}
      </div>

      {error ? (
        <p className="state-message state-message-error" role="alert">
          {error}
        </p>
      ) : null}

      <div className="console-actions">
        <button type="button" onClick={() => void handleConnect()} disabled={!device.online || status === 'connecting'}>
          <PlugZap aria-hidden="true" size={16} />
          {status === 'connecting' ? t('connecting') : t('connect')}
        </button>
      </div>

      <section className="remote-sessions" aria-label={t('remote_sessions')}>
        <div className="panel-title">{t('remote_sessions')}</div>
        {renderSessionGroups()}
      </section>

      <section className="relay-log" aria-label={t('remote_rpc_status')}>
        <div className="panel-title">
          {t('remote_rpc_status')}
          <span className={`relay-status relay-status-${relayStatus}`}>
            {t(`relay_status_${relayStatus}`)}
          </span>
          <span className={`relay-status relay-status-${webRtcStatus}`}>
            {t(`webrtc_status_${webRtcStatus}`)}
          </span>
          <span className={`relay-status transport-status-${activeTransport}`}>
            {t('active_transport')}: {activeTransportLabel()}
          </span>
        </div>
        <div className="rpc-result-grid">
          <div className="rpc-result">
            <div className="summary-label">{t('remote_ping')}</div>
            <pre className="json-preview">{displayRpcResult(pingResult)}</pre>
          </div>
          <div className="rpc-result">
            <div className="summary-label">{t('remote_state')}</div>
            <pre className="json-preview">{displayRpcResult(stateResult)}</pre>
          </div>
          <div className="rpc-result">
            <div className="summary-label">{t('remote_sessions_debug')}</div>
            <pre className="json-preview">{displayRpcResult(sessionsResult)}</pre>
          </div>
        </div>
      </section>

      <section className="signal-log" aria-label={t('signal_messages')}>
        <div className="panel-title">{t('signal_messages')}</div>
        {messages.length === 0 ? <p className="state-message">{t('no_signal_messages')}</p> : null}
        {messages.map((message, index) => (
          <pre className="json-preview" key={index}>
            {displaySignalMessage(message)}
          </pre>
        ))}
      </section>
    </section>
  )
}
