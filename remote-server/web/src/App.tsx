import { Activity, Globe2, LockKeyhole, MonitorUp, PlugZap, Server } from 'lucide-react'
import { useMemo, useState } from 'react'
import { createTranslator, detectLanguage } from './i18n/index.js'
import { supportedLanguages, type SupportedLanguage } from './i18n/messages.js'

const deviceRows = [
  { id: 'mac-studio-01', state: 'online', sessions: 3, relay: 'sha-01' },
  { id: 'win-lab-07', state: 'offline', sessions: 0, relay: 'fra-02' },
  { id: 'linux-edge-03', state: 'online', sessions: 1, relay: 'nrt-01' }
] as const

export function App() {
  const [language, setLanguage] = useState<SupportedLanguage>(() => detectLanguage())
  const t = useMemo(() => createTranslator(language), [language])

  // 首屏使用静态数据占位，后续任务接入真实登录态和设备 API。
  return (
    <main className="console-shell">
      <header className="topbar">
        <div className="brand">
          <MonitorUp aria-hidden="true" size={22} />
          <h1>{t('app_title')}</h1>
        </div>
        <label className="language-picker">
          <Globe2 aria-hidden="true" size={16} />
          <select
            aria-label="language"
            value={language}
            onChange={(event) => setLanguage(event.target.value as SupportedLanguage)}
          >
            {supportedLanguages.map((item) => (
              <option key={item} value={item}>
                {item}
              </option>
            ))}
          </select>
        </label>
      </header>

      <section className="workspace" aria-label={t('devices')}>
        <aside className="login-panel">
          <div className="panel-title">
            <LockKeyhole aria-hidden="true" size={18} />
            <span>{t('login')}</span>
          </div>
          <label>
            {t('email')}
            <input type="email" autoComplete="email" placeholder="operator@example.com" />
          </label>
          <label>
            {t('password')}
            <input type="password" autoComplete="current-password" placeholder="••••••••" />
          </label>
          <button type="button">
            <PlugZap aria-hidden="true" size={16} />
            {t('connect')}
          </button>
        </aside>

        <section className="device-panel">
          <div className="panel-title">
            <Server aria-hidden="true" size={18} />
            <span>{t('devices')}</span>
          </div>
          <div className="device-table" role="table" aria-label={t('devices')}>
            <div className="device-row device-row-head" role="row">
              <span role="columnheader">ID</span>
              <span role="columnheader">{t('state')}</span>
              <span role="columnheader">{t('sessions')}</span>
              <span role="columnheader">{t('relay')}</span>
              <span role="columnheader">{t('connect')}</span>
            </div>
            {deviceRows.map((device) => (
              <div className="device-row" role="row" key={device.id}>
                <span role="cell" className="device-id">
                  {device.id}
                </span>
                <span role="cell" className={`status status-${device.state}`}>
                  <Activity aria-hidden="true" size={14} />
                  {t(device.state)}
                </span>
                <span role="cell">{device.sessions}</span>
                <span role="cell">{device.relay}</span>
                <span role="cell">
                  <button type="button" className="icon-button" aria-label={`${t('connect')} ${device.id}`}>
                    <PlugZap aria-hidden="true" size={15} />
                  </button>
                </span>
              </div>
            ))}
          </div>
        </section>
      </section>
    </main>
  )
}
