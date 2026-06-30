import { ArrowLeft, PlugZap, TerminalSquare } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteDevice } from '../api/devicesApi.js'
import { getStableClientId } from './clientId.js'
import type { ConnectionClient, ConnectionClientOptions } from './connectionClient.js'
import type { RelayClient, RelayClientOptions } from './relayTransport.js'
import { RemoteSessionGroupsView } from './RemoteSessionGroupsView.js'
import {
  createRemoteDeviceSessionController,
  type RemoteDeviceSessionController,
  type RemoteDeviceSessionSnapshot,
  type RpcResultState
} from './remoteDeviceSessionController.js'
import { isProjectGroupPage } from './remoteSessionTypes.js'
import type { WebRtcTransport, WebRtcTransportOptions } from './webrtcTransport.js'

type ConnectionsApi = {
  create(deviceId: string, clientId: string): Promise<ConnectionCreateResult>
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

function emptyRpcResult(): RpcResultState {
  return { status: 'idle', value: null }
}

function createIdleSnapshot(): RemoteDeviceSessionSnapshot {
  return {
    connectionStatus: 'idle',
    relayStatus: 'idle',
    webRtcStatus: 'idle',
    activeTransport: 'idle',
    connectionId: null,
    error: null,
    pingResult: emptyRpcResult(),
    stateResult: emptyRpcResult(),
    sessionsResult: emptyRpcResult(),
    diagnostics: {
      relay: emptyRpcResult(),
      webrtc: emptyRpcResult()
    },
    diagnosticReport: null,
    diagnosticRunning: false
  }
}

function displaySignalMessage(value: unknown): string {
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function displayJson(t: (key: string) => string, value: unknown): string {
  if (value === null) return t('waiting_for_relay')
  if (typeof value === 'string') return value
  try {
    return JSON.stringify(value, null, 2)
  } catch {
    return String(value)
  }
}

function displayControllerText(t: (key: string) => string, value: unknown): string {
  if (value === 'Plain RPC client closed') return t('remote_rpc_failed')
  if (typeof value !== 'string') return displayJson(t, value)

  const translated = t(value)
  return translated === value ? value : translated
}

function displayRpcResult(t: (key: string) => string, result: RpcResultState): string {
  if (result.status === 'idle') return t('waiting_for_relay')
  if (result.status === 'loading') return t('waiting_for_response')
  if (result.status === 'error') return displayControllerText(t, result.value)
  return displayJson(t, result.value)
}

function renderSessionGroups(t: (key: string) => string, sessionsResult: RpcResultState) {
  if (sessionsResult.status === 'idle') return <p className="state-message">{t('waiting_for_relay')}</p>
  if (sessionsResult.status === 'loading') return <p className="state-message">{t('waiting_for_response')}</p>
  if (sessionsResult.status === 'error' || !isProjectGroupPage(sessionsResult.value)) {
    return (
      <p className="state-message state-message-error" role="alert">
        {t('remote_sessions_failed')}
      </p>
    )
  }

  return <RemoteSessionGroupsView page={sessionsResult.value} emptyText={t('remote_sessions_empty')} />
}

export function DeviceConsolePage({
  device,
  connectionsApi,
  createConnection,
  createRelay,
  createWebRtc,
  autoConnect = false,
  t,
  onBack
}: DeviceConsolePageProps) {
  const clientId = useMemo(() => getStableClientId(), [])
  const [snapshot, setSnapshot] = useState<RemoteDeviceSessionSnapshot>(() => createIdleSnapshot())
  const [messages, setMessages] = useState<unknown[]>([])
  const autoConnectStartedRef = useRef(false)

  const controller = useMemo<RemoteDeviceSessionController>(() => {
    return createRemoteDeviceSessionController({
      device,
      connectionsApi,
      clientId,
      createConnection,
      createRelay,
      createWebRtc,
      onSnapshot: setSnapshot,
      onSignalMessage: (message) => {
        setMessages((current) => [message, ...current].slice(0, 20))
      }
    })
  }, [clientId, connectionsApi, createConnection, createRelay, createWebRtc, device])

  useEffect(() => {
    setSnapshot(createIdleSnapshot())
    setMessages([])
    return () => {
      controller.close()
    }
  }, [controller])

  useEffect(() => {
    if (!autoConnect || autoConnectStartedRef.current || !device.online) return
    autoConnectStartedRef.current = true
    // 从设备列表点击“连接”进入控制台时，自动发起一次真实连接。
    void controller.connect()
  }, [autoConnect, controller, device.online])

  function handleConnect() {
    if (!device.online || snapshot.connectionStatus === 'connecting') return
    setMessages([])
    void controller.connect()
  }

  const activeTransportLabel = t(`active_transport_${snapshot.activeTransport}`)

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
          <strong>{t(`connection_status_${snapshot.connectionStatus}`)}</strong>
        </div>
        {snapshot.connectionId ? (
          <div>
            <span className="summary-label">{t('connection_id')}</span>
            <strong className="device-id">{snapshot.connectionId}</strong>
          </div>
        ) : null}
      </div>

      {snapshot.error ? (
        <p className="state-message state-message-error" role="alert">
          {displayControllerText(t, snapshot.error)}
        </p>
      ) : null}

      <div className="console-actions">
        <button
          type="button"
          onClick={handleConnect}
          disabled={!device.online || snapshot.connectionStatus === 'connecting'}
        >
          <PlugZap aria-hidden="true" size={16} />
          {snapshot.connectionStatus === 'connecting' ? t('connecting') : t('connect')}
        </button>
      </div>

      <section className="remote-sessions" aria-label={t('remote_sessions')}>
        <div className="panel-title">{t('remote_sessions')}</div>
        {renderSessionGroups(t, snapshot.sessionsResult)}
      </section>

      <section className="relay-log" aria-label={t('remote_rpc_status')}>
        <div className="panel-title">
          {t('remote_rpc_status')}
          <span className={`relay-status relay-status-${snapshot.relayStatus}`}>
            {t(`relay_status_${snapshot.relayStatus}`)}
          </span>
          <span className={`relay-status relay-status-${snapshot.webRtcStatus}`}>
            {t(`webrtc_status_${snapshot.webRtcStatus}`)}
          </span>
          <span className={`relay-status transport-status-${snapshot.activeTransport}`}>
            {t('active_transport')}: {activeTransportLabel}
          </span>
        </div>
        <div className="rpc-result-grid">
          <div className="rpc-result">
            <div className="summary-label">{t('remote_ping')}</div>
            <pre className="json-preview">{displayRpcResult(t, snapshot.pingResult)}</pre>
          </div>
          <div className="rpc-result">
            <div className="summary-label">{t('remote_state')}</div>
            <pre className="json-preview">{displayRpcResult(t, snapshot.stateResult)}</pre>
          </div>
          <div className="rpc-result">
            <div className="summary-label">{t('remote_sessions_debug')}</div>
            <pre className="json-preview">{displayRpcResult(t, snapshot.sessionsResult)}</pre>
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
