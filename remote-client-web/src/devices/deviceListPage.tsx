import { LogoutOutlined, ReloadOutlined, SelectOutlined } from '@ant-design/icons'
import { Button, Empty, List, Space, Tag } from 'antd'

import type { RemoteDevice } from '../api/devicesApi.js'
import { LanguageSelect } from '../i18n/LanguageSelect.js'
import { useI18n } from '../i18n/index.js'

export type DeviceListPageProps = {
  devices: RemoteDevice[]
  loading: boolean
  error?: string | null
  userEmail: string
  onRefresh: () => void
  onLogout: () => void
  onOpenDevice: (device: RemoteDevice) => void
}

function formatLastSeen(value: string | null, fallback: string): string {
  if (!value) return fallback
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return fallback
  return date.toLocaleString()
}

function summarizeCapabilities(value: unknown, fallback: string): string {
  if (Array.isArray(value)) {
    const text = value.map((item) => String(item)).filter(Boolean).join(', ')
    return text || fallback
  }

  if (value && typeof value === 'object') {
    // capabilities 由后端透传，列表页只展示顶层 key，详细解释留给 session 页面。
    const keys = Object.keys(value)
    return keys.length > 0 ? keys.join(', ') : fallback
  }

  if (value == null) return fallback
  return String(value)
}

export function DeviceListPage({
  devices,
  loading,
  error,
  userEmail,
  onRefresh,
  onLogout,
  onOpenDevice
}: DeviceListPageProps) {
  const { t } = useI18n()

  return (
    <section className="device-page" aria-labelledby="device-list-title">
      <header className="page-toolbar">
        <div>
          <h1 id="device-list-title">{t('devices_title')}</h1>
          <p>{t('devices_description')}</p>
          <span className="user-email">{userEmail}</span>
        </div>
        <Space wrap>
          <LanguageSelect />
          <Button icon={<ReloadOutlined />} onClick={onRefresh} loading={loading}>
            {t('refresh_button')}
          </Button>
          <Button icon={<LogoutOutlined />} onClick={onLogout}>
            {t('logout_button')}
          </Button>
        </Space>
      </header>

      {error ? <p className="page-error" role="alert">{error}</p> : null}

      <List
        className="device-list"
        loading={loading}
        dataSource={devices}
        locale={{ emptyText: <Empty description={t('devices_empty')} /> }}
        renderItem={(device) => (
          <List.Item
            actions={[
              <Button
                key="open"
                type="primary"
                icon={<SelectOutlined />}
                aria-label={`${t('open_device_button')} ${device.name}`}
                disabled={!device.online}
                onClick={() => onOpenDevice(device)}
              >
                {t('open_device_button')}
              </Button>
            ]}
          >
            <List.Item.Meta
              title={
                <Space wrap>
                  <span>{device.name}</span>
                  <Tag color={device.online ? 'green' : 'default'}>
                    {device.online ? t('device_online') : t('device_offline')}
                  </Tag>
                </Space>
              }
              description={
                <dl className="device-meta">
                  <div>
                    <dt>{t('device_last_seen')}</dt>
                    <dd>{formatLastSeen(device.last_seen_at, t('device_never_seen'))}</dd>
                  </div>
                  <div>
                    <dt>{t('device_capabilities')}</dt>
                    <dd>{summarizeCapabilities(device.capabilities, t('device_capabilities_empty'))}</dd>
                  </div>
                </dl>
              }
            />
          </List.Item>
        )}
      />
    </section>
  )
}
