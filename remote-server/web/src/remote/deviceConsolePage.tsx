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
import { createPlainRpcClient, type PlainRpcClient } from './plainRpcClient.js'
import {
  buildRelaySocketUrl,
  createRelayClient,
  type RelayClient,
  type RelayClientOptions
} from './relayTransport.js'

type ConnectionsApi = {
  create(deviceId: string, clientId: string): Promise<ConnectionCreateResult>
}

type RpcResultState = {
  status: 'idle' | 'loading' | 'ready' | 'error'
  value: unknown
}

type DeviceConsolePageProps = {
  device: RemoteDevice
  connectionsApi: ConnectionsApi
  createConnection?: (options: ConnectionClientOptions) => ConnectionClient
  createRelay?: (options: RelayClientOptions) => RelayClient
  t: (key: string) => string
  onBack(): void
}

const CLIENT_ID_KEY = 'niuma.remote.client_id'
let memoryClientId: string | null = null

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

export function DeviceConsolePage({
  device,
  connectionsApi,
  createConnection = createConnectionClient,
  createRelay = createRelayClient,
  t,
  onBack
}: DeviceConsolePageProps) {
  const clientId = useMemo(() => getStableClientId(), [])
  const [status, setStatus] = useState<ConnectionStatus | 'idle'>('idle')
  const [relayStatus, setRelayStatus] = useState<'idle' | 'connecting' | 'open' | 'closed' | 'error'>('idle')
  const [connectionId, setConnectionId] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [messages, setMessages] = useState<unknown[]>([])
  const [pingResult, setPingResult] = useState<RpcResultState>({ status: 'idle', value: null })
  const [stateResult, setStateResult] = useState<RpcResultState>({ status: 'idle', value: null })
  const [sessionsResult, setSessionsResult] = useState<RpcResultState>({ status: 'idle', value: null })
  const socketRef = useRef<ConnectionClient | null>(null)
  const relayRef = useRef<RelayClient | null>(null)
  const rpcRef = useRef<PlainRpcClient | null>(null)
  const activeConnectionRef = useRef(0)
  const mountedRef = useRef(false)

  useEffect(() => {
    mountedRef.current = true
    return () => {
      mountedRef.current = false
      activeConnectionRef.current += 1
      socketRef.current?.close()
      rpcRef.current?.close()
      relayRef.current?.close()
      socketRef.current = null
      rpcRef.current = null
      relayRef.current = null
    }
  }, [])

  function isActiveConnection(connectionId: number): boolean {
    return mountedRef.current && activeConnectionRef.current === connectionId
  }

  function resetRelayConsole() {
    setRelayStatus('idle')
    setPingResult({ status: 'idle', value: null })
    setStateResult({ status: 'idle', value: null })
    setSessionsResult({ status: 'idle', value: null })
  }

  function closeRelayConsole() {
    rpcRef.current?.close()
    relayRef.current?.close()
    rpcRef.current = null
    relayRef.current = null
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

  function openRelayConsole(result: ConnectionCreateResult, activeConnectionId: number) {
    closeRelayConsole()
    resetRelayConsole()
    setRelayStatus('connecting')

    const relayUrl = buildRelaySocketUrl(result.relay_url || result.signaling_url || window.location.origin, {
      connection_id: result.connection_id,
      connection_token: result.connection_token,
      side: 'client'
    })
    let relayClient: RelayClient
    const rpcClient = createPlainRpcClient({
      timeoutMs: 10_000,
      send: (value) => {
        relayClient.send(value)
      }
    })

    function updateRpcResult(
      method: string,
      setResult: Dispatch<SetStateAction<RpcResultState>>
    ) {
      setResult({ status: 'loading', value: null })
      void rpcClient
        .request(method)
        .then((value) => {
          if (isActiveConnection(activeConnectionId)) setResult({ status: 'ready', value })
        })
        .catch(() => {
          if (isActiveConnection(activeConnectionId)) {
            setResult({ status: 'error', value: t('remote_rpc_failed') })
          }
        })
    }

    function closeRpcForRelayEvent(options: { closeRelay: boolean }) {
      // relay 断开后本轮请求已不会再收到响应，立即关闭 RPC 以清理 pending。
      rpcClient.close()
      if (options.closeRelay) relayClient.close()
      if (rpcRef.current === rpcClient) rpcRef.current = null
      if (relayRef.current === relayClient) relayRef.current = null
    }

    relayClient = createRelay({
      url: relayUrl,
      connectionId: result.connection_id,
      onOpen: () => {
        if (!isActiveConnection(activeConnectionId)) return
        setRelayStatus('open')
        updateRpcResult('rpc.ping', setPingResult)
        updateRpcResult('state.get', setStateResult)
        updateRpcResult('session.list', setSessionsResult)
      },
      onPayload: (value) => {
        if (isActiveConnection(activeConnectionId)) rpcClient.handle(value)
      },
      onClose: () => {
        if (!isActiveConnection(activeConnectionId)) return
        closeRpcForRelayEvent({ closeRelay: false })
        setRelayStatus('closed')
      },
      onError: () => {
        if (!isActiveConnection(activeConnectionId)) return
        closeRpcForRelayEvent({ closeRelay: true })
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
      socketRef.current = createConnection({
        url: socketUrl,
        onStatus: (nextStatus) => {
          if (!isActiveConnection(activeConnectionId)) return
          setStatus(nextStatus)
          if (nextStatus === 'accepted' && !relayStarted) {
            relayStarted = true
            openRelayConsole(result, activeConnectionId)
          }
        },
        onMessage: (value) => {
          if (isActiveConnection(activeConnectionId)) {
            setMessages((current) => [value, ...current].slice(0, 20))
          }
        }
      })
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

      <section className="relay-log" aria-label={t('remote_rpc_status')}>
        <div className="panel-title">
          {t('remote_rpc_status')}
          <span className={`relay-status relay-status-${relayStatus}`}>
            {t(`relay_status_${relayStatus}`)}
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
            <div className="summary-label">{t('remote_sessions')}</div>
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
