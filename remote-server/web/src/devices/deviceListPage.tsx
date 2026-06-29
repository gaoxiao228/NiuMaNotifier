import { Activity, PlugZap, RefreshCw, Server, TerminalSquare } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import type { RemoteDevice } from '../api/devicesApi.js'
import { HttpError } from '../api/httpClient.js'
import { getStableClientId } from '../remote/clientId.js'
import { RemoteSessionGroupsView } from '../remote/RemoteSessionGroupsView.js'
import {
  createRemoteDeviceSessionController,
  type RemoteDeviceSessionController,
  type RemoteDeviceSessionSnapshot,
  type RpcResultState
} from '../remote/remoteDeviceSessionController.js'
import { isProjectGroupPage } from '../remote/remoteSessionTypes.js'
import { ApiError } from '../shared/envelope.js'
import { toDisplayErrorMessage } from '../shared/errorMessage.js'

type DeviceListApi = {
  list(): Promise<{ list: RemoteDevice[] }>
}

type ConnectionsApi = {
  create(deviceId: string, clientId: string): Promise<ConnectionCreateResult>
}

type RemoteSessionControllerFactory = typeof createRemoteDeviceSessionController

type DeviceListPageProps = {
  devicesApi: DeviceListApi
  connectionsApi: ConnectionsApi
  clientId?: string
  createRemoteSessionController?: RemoteSessionControllerFactory
  t: (key: string) => string
  onSelectDevice(device: RemoteDevice): void
  onUnauthorized?(): void
}

const AUTH_INVALID_CODES = new Set([200001, 200002, 200003])

function isUnauthorizedError(err: unknown): boolean {
  // 服务端会用不同业务码表达未登录、Token 无效和 Token 过期，这些都需要回到登录页。
  return (err instanceof ApiError && AUTH_INVALID_CODES.has(err.code)) || (err instanceof HttpError && err.status === 401)
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
    }
  }
}

function displayRpcStatus(t: (key: string) => string, result: RpcResultState): string {
  return t(`rpc_result_status_${result.status}`)
}

function displayControllerError(t: (key: string) => string, error: string): string {
  const translated = t(error)
  return translated === error ? error : translated
}

function renderSessionGroups(t: (key: string) => string, sessionsResult: RpcResultState) {
  if (sessionsResult.status === 'idle') return <p className="state-message">{t('remote_sessions_waiting_for_connection')}</p>
  if (sessionsResult.status === 'loading') return <p className="state-message">{t('waiting_for_response')}</p>
  if (sessionsResult.status === 'error') {
    return (
      <p className="state-message state-message-error" role="alert">
        {t('remote_sessions_failed')}
      </p>
    )
  }
  if (!isProjectGroupPage(sessionsResult.value)) {
    return (
      <p className="state-message state-message-error" role="alert">
        {t('remote_sessions_invalid')}
      </p>
    )
  }

  return <RemoteSessionGroupsView page={sessionsResult.value} emptyText={t('remote_sessions_empty')} />
}

export function DeviceListPage({
  devicesApi,
  connectionsApi,
  clientId,
  createRemoteSessionController = createRemoteDeviceSessionController,
  t,
  onSelectDevice,
  onUnauthorized
}: DeviceListPageProps) {
  const [devices, setDevices] = useState<RemoteDevice[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)
  const [autoSessionSnapshot, setAutoSessionSnapshot] = useState<RemoteDeviceSessionSnapshot>(() => createIdleSnapshot())
  const requestIdRef = useRef(0)
  const mountedRef = useRef(true)
  const autoControllerRef = useRef<RemoteDeviceSessionController | null>(null)
  const autoClientId = useMemo(() => clientId ?? getStableClientId(), [clientId])
  const firstOnlineDeviceId = devices.find((device) => device.online)?.id ?? null
  const firstOnlineDevice = firstOnlineDeviceId ? devices.find((device) => device.id === firstOnlineDeviceId) ?? null : null

  const loadDevices = useCallback(async () => {
    const requestId = requestIdRef.current + 1
    requestIdRef.current = requestId
    setLoading(true)
    setError(null)
    try {
      const response = await devicesApi.list()
      if (!mountedRef.current || requestId !== requestIdRef.current) return
      setDevices(response.list)
    } catch (err) {
      if (!mountedRef.current || requestId !== requestIdRef.current) return
      if (isUnauthorizedError(err)) {
        onUnauthorized?.()
        return
      }
      setError(toDisplayErrorMessage(t, err, 'error'))
    } finally {
      if (!mountedRef.current || requestId !== requestIdRef.current) return
      setLoading(false)
    }
  }, [devicesApi, onUnauthorized, t])

  useEffect(() => {
    mountedRef.current = true
    void loadDevices()
    return () => {
      mountedRef.current = false
      requestIdRef.current += 1
    }
  }, [loadDevices])

  useEffect(() => {
    // 列表页只维护第一个在线设备的只读会话视图，点击设备仍进入完整控制台。
    const device = firstOnlineDevice
    setAutoSessionSnapshot(createIdleSnapshot())
    if (!device) {
      autoControllerRef.current = null
      return
    }

    let closed = false
    const controller = createRemoteSessionController({
      device,
      connectionsApi,
      clientId: autoClientId,
      onSnapshot: (snapshot) => {
        if (!closed) setAutoSessionSnapshot(snapshot)
      }
    })
    autoControllerRef.current = controller
    void controller.connect().catch(() => {
      if (!closed) {
        setAutoSessionSnapshot({
          ...createIdleSnapshot(),
          connectionStatus: 'error',
          error: t('connection_failed')
        })
      }
    })

    return () => {
      closed = true
      if (autoControllerRef.current === controller) autoControllerRef.current = null
      controller.close()
    }
  }, [autoClientId, connectionsApi, createRemoteSessionController, firstOnlineDeviceId])

  function renderRemoteSessionPanel() {
    const snapshot = autoSessionSnapshot
    const device = firstOnlineDevice
    return (
      <section className="remote-sessions device-list-remote-sessions" aria-label={t('remote_sessions')}>
        <div className="panel-title">
          <TerminalSquare aria-hidden="true" size={18} />
          <span>{t('remote_sessions')}</span>
        </div>

        {!device ? <p className="state-message">{t('remote_sessions_no_online_device')}</p> : null}
        {device ? (
          <>
            <div className="console-summary remote-session-summary" aria-label={t('remote_sessions')}>
              <div>
                <span className="summary-label">{t('auto_remote_session_device')}</span>
                <strong>{device.name || device.id}</strong>
              </div>
              <div>
                <span className="summary-label">{t('identifier')}</span>
                <strong className="device-id">{device.id}</strong>
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
                {displayControllerError(t, snapshot.error)}
              </p>
            ) : null}

            {renderSessionGroups(t, snapshot.sessionsResult)}

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
                  {t('active_transport')}: {t(`active_transport_${snapshot.activeTransport}`)}
                </span>
              </div>
              <div className="rpc-result-grid diagnostic-result-grid">
                <div className="rpc-result">
                  <div className="summary-label">{t('relay_diagnostics')}</div>
                  <strong>{displayRpcStatus(t, snapshot.diagnostics.relay)}</strong>
                </div>
                <div className="rpc-result">
                  <div className="summary-label">{t('webrtc_diagnostics')}</div>
                  <strong>{displayRpcStatus(t, snapshot.diagnostics.webrtc)}</strong>
                </div>
              </div>
            </section>
          </>
        ) : null}
      </section>
    )
  }

  return (
    <section className="device-panel">
      <div className="panel-title device-panel-heading">
        <span className="panel-title-label">
          <Server aria-hidden="true" size={18} />
          <span>{t('devices')}</span>
        </span>
        <button type="button" className="secondary-button" onClick={() => void loadDevices()} disabled={loading}>
          <RefreshCw aria-hidden="true" size={15} />
          {t('refresh')}
        </button>
      </div>

      {loading ? <p className="state-message">{t('loading')}</p> : null}
      {error ? (
        <p className="state-message state-message-error" role="alert">
          {error}
        </p>
      ) : null}
      {!loading && !error && devices.length === 0 ? <p className="state-message">{t('no_devices')}</p> : null}

      {devices.length > 0 ? (
        <div className="device-table" role="table" aria-label={t('devices')}>
          <div className="device-row device-row-head" role="row">
            <span role="columnheader">{t('identifier')}</span>
            <span role="columnheader">{t('state')}</span>
            <span role="columnheader">{t('last_seen')}</span>
            <span role="columnheader">{t('connect')}</span>
          </div>
          {devices.map((device) => (
            <div className="device-row" role="row" key={device.id}>
              <span role="cell" className="device-id">
                {device.name || device.id}
              </span>
              <span role="cell" className={`status status-${device.online ? 'online' : 'offline'}`}>
                <Activity aria-hidden="true" size={14} />
                {t(device.online ? 'online' : 'offline')}
              </span>
              <span role="cell">{device.last_seen_at ?? t('never_seen')}</span>
              <span role="cell">
                <button
                  type="button"
                  className="icon-button"
                  aria-label={`${t('connect')} ${device.name || device.id}`}
                  disabled={!device.online}
                  onClick={() => onSelectDevice(device)}
                >
                  <PlugZap aria-hidden="true" size={15} />
                </button>
              </span>
            </div>
          ))}
        </div>
      ) : null}

      {renderRemoteSessionPanel()}
    </section>
  )
}
