import { ArrowLeftOutlined, LogoutOutlined } from '@ant-design/icons'
import { Alert, Button, Space, Tag } from 'antd'

import type { RemoteDevice } from '../api/devicesApi.js'
import { useI18n } from '../i18n/index.js'
import type {
  RemoteDeviceActiveTransport,
  RemoteDeviceSessionSnapshot,
  RemoteDeviceTransportStatus,
  RpcResultState
} from '../remote/remoteDeviceSessionController.js'
import { isProjectGroupPage, type RemoteSessionProjectGroupPage } from '../remote/remoteSessionTypes.js'
import { SessionGroupsView } from './SessionGroupsView.js'

export type SessionConsolePageProps = {
  device: RemoteDevice
  snapshot: RemoteDeviceSessionSnapshot | null
  onBack: () => void
  onLogout: () => void
}

type SessionResultView = {
  page: RemoteSessionProjectGroupPage | null
  loading: boolean
  error: string | null
}

function idleResult(): RpcResultState {
  return { status: 'idle', value: null }
}

function fallbackSnapshot(): RemoteDeviceSessionSnapshot {
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
    },
    diagnosticReport: null,
    diagnosticRunning: false
  }
}

function transportStatusColor(status: string): string {
  switch (status) {
    case 'open':
    case 'accepted':
      return 'success'
    case 'connecting':
      return 'processing'
    case 'error':
    case 'rejected':
      return 'error'
    case 'closed':
      return 'default'
    default:
      return 'default'
  }
}

function localizedTransportStatus(t: ReturnType<typeof useI18n>['t'], status: string): string {
  switch (status) {
    case 'idle':
      return t('connection_status_idle')
    case 'connecting':
      return t('connection_status_connecting')
    case 'accepted':
      return t('connection_status_accepted')
    case 'rejected':
      return t('connection_status_rejected')
    case 'open':
      return t('connection_status_open')
    case 'closed':
      return t('connection_status_closed')
    case 'error':
      return t('connection_status_error')
    default:
      return status
  }
}

function localizedActiveTransport(t: ReturnType<typeof useI18n>['t'], transport: RemoteDeviceActiveTransport): string {
  switch (transport) {
    case 'relay':
      return t('transport_relay')
    case 'webrtc':
      return t('transport_webrtc')
    default:
      return t('transport_idle')
  }
}

function readSessionsResult(t: ReturnType<typeof useI18n>['t'], result: RpcResultState): SessionResultView {
  if (result.status === 'idle' || result.status === 'loading') {
    return { page: null, loading: true, error: null }
  }
  if (result.status === 'error') {
    return {
      page: null,
      loading: false,
      error: typeof result.value === 'string' ? result.value : t('session_list_error')
    }
  }
  if (!isProjectGroupPage(result.value)) {
    // stream 数据来自远端 Local API；只做渲染所需的最小结构保护。
    return { page: null, loading: false, error: t('session_list_invalid') }
  }
  return { page: result.value, loading: false, error: null }
}

function StatusItem({
  label,
  value,
  rawValue
}: {
  label: string
  value: string
  rawValue: string | RemoteDeviceTransportStatus
}) {
  return (
    <div className="session-status-item">
      <span>{label}</span>
      <Tag color={transportStatusColor(rawValue)}>{value}</Tag>
    </div>
  )
}

export function SessionConsolePage({ device, snapshot, onBack, onLogout }: SessionConsolePageProps) {
  const { t } = useI18n()
  const currentSnapshot = snapshot ?? fallbackSnapshot()
  const sessions = readSessionsResult(t, currentSnapshot.sessionsResult)

  return (
    <section className="session-console-page" aria-labelledby="session-console-title">
      <header className="session-console-toolbar">
        <div className="session-toolbar-main">
          <Button icon={<ArrowLeftOutlined />} aria-label={t('back_to_devices_button')} onClick={onBack}>
            {t('back_to_devices_button')}
          </Button>
          <div>
            <h1 id="session-console-title">{device.name}</h1>
            <span className="session-device-id">{device.id}</span>
          </div>
        </div>
        <Button icon={<LogoutOutlined />} aria-label={t('logout_button')} onClick={onLogout}>
          {t('logout_button')}
        </Button>
      </header>

      <div className="session-connection-strip" aria-label={t('connection_summary_label')}>
        <StatusItem
          label={t('connection_status_label')}
          rawValue={currentSnapshot.connectionStatus}
          value={localizedTransportStatus(t, currentSnapshot.connectionStatus)}
        />
        <StatusItem
          label={t('relay_status_label')}
          rawValue={currentSnapshot.relayStatus}
          value={localizedTransportStatus(t, currentSnapshot.relayStatus)}
        />
        <StatusItem
          label={t('webrtc_status_label')}
          rawValue={currentSnapshot.webRtcStatus}
          value={localizedTransportStatus(t, currentSnapshot.webRtcStatus)}
        />
        <div className="session-status-item">
          <span>{t('active_transport_label')}</span>
          <Tag color={currentSnapshot.activeTransport === 'idle' ? 'default' : 'blue'}>
            {localizedActiveTransport(t, currentSnapshot.activeTransport)}
          </Tag>
        </div>
      </div>

      {currentSnapshot.error ? (
        <Alert className="session-error" type="error" showIcon message={t('connection_error_label')} description={currentSnapshot.error} />
      ) : null}

      <SessionGroupsView page={sessions.page} loading={sessions.loading} error={sessions.error} />
    </section>
  )
}
