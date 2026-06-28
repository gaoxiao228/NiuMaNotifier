import { Activity, PlugZap, RefreshCw, Server } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'

import type { RemoteDevice } from '../api/devicesApi.js'

type DeviceListApi = {
  list(): Promise<{ list: RemoteDevice[] }>
}

type DeviceListPageProps = {
  devicesApi: DeviceListApi
  t: (key: string) => string
  onSelectDevice(device: RemoteDevice): void
}

export function DeviceListPage({ devicesApi, t, onSelectDevice }: DeviceListPageProps) {
  const [devices, setDevices] = useState<RemoteDevice[]>([])
  const [loading, setLoading] = useState(true)
  const [error, setError] = useState<string | null>(null)

  const loadDevices = useCallback(async () => {
    setLoading(true)
    setError(null)
    try {
      const response = await devicesApi.list()
      setDevices(response.list)
    } catch (err) {
      setError(err instanceof Error ? err.message : t('error'))
    } finally {
      setLoading(false)
    }
  }, [devicesApi, t])

  useEffect(() => {
    void loadDevices()
  }, [loadDevices])

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
          {t('error')}: {error}
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
    </section>
  )
}
