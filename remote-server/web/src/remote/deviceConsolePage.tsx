import { Activity, ArrowLeft, PlugZap, TerminalSquare } from 'lucide-react'
import { useEffect, useMemo, useRef, useState } from 'react'

import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteDevice } from '../api/devicesApi.js'
import { getStableClientId } from './clientId.js'
import type { ConnectionClient, ConnectionClientOptions } from './connectionClient.js'
import type { RelayClient, RelayClientOptions } from './relayTransport.js'
import { RemoteSessionGroupsView } from './RemoteSessionGroupsView.js'
import type { DiagnosticReport } from './diagnostics.js'
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

function renderDiagnosticReport(t: (key: string) => string, report: DiagnosticReport | null) {
  if (!report) return null

  return (
    <section className={`diagnostic-report diagnostic-report-${report.overall}`} aria-label={t('diagnostics')}>
      <div className="panel-title">{t('diagnostics')}</div>
      <p className="diagnostic-summary">{t(report.summary)}</p>
      <dl className="diagnostic-step-list">
        {report.steps.map((step) => (
          <div className="diagnostic-step" key={step.key}>
            <dt>{t(step.title)}</dt>
            <dd>
              <span className={`diagnostic-step-status diagnostic-step-status-${step.status}`}>
                {t(`diagnostic_status_${step.status}`)}
              </span>
              {typeof step.duration_ms === 'number' ? <span>{step.duration_ms}ms</span> : null}
              {step.message ? <span>{t(step.message)}</span> : null}
            </dd>
          </div>
        ))}
      </dl>
    </section>
  )
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
  const autoConnectStartedRef = useRef(false)

  const controller = useMemo<RemoteDeviceSessionController>(() => {
    return createRemoteDeviceSessionController({
      device,
      connectionsApi,
      clientId,
      createConnection,
      createRelay,
      createWebRtc,
      onSnapshot: setSnapshot
    })
  }, [clientId, connectionsApi, createConnection, createRelay, createWebRtc, device])

  useEffect(() => {
    setSnapshot(createIdleSnapshot())
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
    void controller.connect()
  }

  function handleRunDiagnostics() {
    if (!device.online || snapshot.connectionStatus === 'connecting' || snapshot.diagnosticRunning) return
    // 诊断复用 controller 的远程连接流程，页面只负责触发和渲染报告。
    void controller.runDiagnostics()
  }

  const activeTransportLabel = t(`active_transport_${snapshot.activeTransport}`)

  return (
    <section className="device-panel console-panel">
      <div className="panel-title device-panel-heading">
        <span className="panel-title-label">
          <TerminalSquare aria-hidden="true" size={18} />
          <span>{t('remote_sessions')}</span>
        </span>
        <button type="button" className="secondary-button" onClick={onBack}>
          <ArrowLeft aria-hidden="true" size={15} />
          {t('back_to_devices')}
        </button>
      </div>

      <div className="remote-session-topbar" aria-label={t('remote_connection_summary')}>
        <div className="remote-session-device">
          <strong>{device.name || device.id}</strong>
          <span className={`status-${device.online ? 'online' : 'offline'}`}>
            {t(device.online ? 'online' : 'offline')}
          </span>
          {snapshot.connectionId ? <span className="device-id">{snapshot.connectionId}</span> : null}
        </div>
        <div className="remote-session-connection">
          <span className="compact-status">
            <span className="summary-label">{t('connection_status')}</span>
            <strong>{t(`connection_status_${snapshot.connectionStatus}`)}</strong>
          </span>
          <span className={`relay-status transport-status-${snapshot.activeTransport}`}>
            {t('active_transport')}: {activeTransportLabel}
          </span>
          <span className={`relay-status relay-status-${snapshot.relayStatus}`}>
            {t(`relay_status_${snapshot.relayStatus}`)}
          </span>
          <span className={`relay-status relay-status-${snapshot.webRtcStatus}`}>
            {t(`webrtc_status_${snapshot.webRtcStatus}`)}
          </span>
        </div>
        <div className="console-actions remote-session-actions">
          <button
            type="button"
            onClick={handleConnect}
            disabled={!device.online || snapshot.connectionStatus === 'connecting'}
          >
            <PlugZap aria-hidden="true" size={16} />
            {snapshot.connectionStatus === 'connecting' ? t('connecting') : t('connect')}
          </button>
          <button
            type="button"
            className="secondary-button"
            onClick={handleRunDiagnostics}
            disabled={!device.online || snapshot.connectionStatus === 'connecting' || snapshot.diagnosticRunning}
          >
            <Activity aria-hidden="true" size={16} />
            {snapshot.diagnosticRunning ? t('diagnostics_running') : t('run_diagnostics')}
          </button>
        </div>
      </div>

      {snapshot.error ? (
        <p className="state-message state-message-error" role="alert">
          {displayControllerText(t, snapshot.error)}
        </p>
      ) : null}

      {renderDiagnosticReport(t, snapshot.diagnosticReport)}

      <section className="remote-sessions remote-sessions-primary" aria-label={t('remote_sessions')}>
        <div className="panel-title">{t('remote_sessions')}</div>
        {renderSessionGroups(t, snapshot.sessionsResult)}
      </section>
    </section>
  )
}
