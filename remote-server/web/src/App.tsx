import { Globe2, LockKeyhole, MonitorUp, PlugZap } from 'lucide-react'
import type { FormEvent } from 'react'
import { useEffect, useMemo, useState } from 'react'
import { createAuthApi } from './api/authApi.js'
import { createConnectionsApi } from './api/connectionsApi.js'
import type { RemoteDevice } from './api/devicesApi.js'
import { createDevicesApi } from './api/devicesApi.js'
import { createHttpClient } from './api/httpClient.js'
import { createLocalStorageAuthStore } from './auth/authStore.js'
import { DeviceListPage } from './devices/deviceListPage.js'
import { createTranslator, detectLanguage } from './i18n/index.js'
import { supportedLanguages, type SupportedLanguage } from './i18n/messages.js'
import { DeviceConsolePage } from './remote/deviceConsolePage.js'
import { toDisplayErrorMessage } from './shared/errorMessage.js'

export function App() {
  const [language, setLanguage] = useState<SupportedLanguage>(() => detectLanguage())
  const t = useMemo(() => createTranslator(language), [language])
  const authStore = useMemo(() => createLocalStorageAuthStore(), [])
  const http = useMemo(() => createHttpClient(authStore), [authStore])
  const authApi = useMemo(() => createAuthApi(http), [http])
  const devicesApi = useMemo(() => createDevicesApi(http), [http])
  const connectionsApi = useMemo(() => createConnectionsApi(http), [http])
  const [token, setToken] = useState<string | null>(() => authStore.getToken())
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [loginLoading, setLoginLoading] = useState(false)
  const [loginError, setLoginError] = useState<string | null>(null)
  const [selectedDevice, setSelectedDevice] = useState<RemoteDevice | null>(null)

  useEffect(() => {
    document.title = t('app_title')
    document.documentElement.lang = language
  }, [language, t])

  async function handleLogin(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    setLoginLoading(true)
    setLoginError(null)
    try {
      const response = await authApi.login(email, password)
      authStore.setToken(response.access_token)
      setToken(response.access_token)
    } catch (err) {
      const message = toDisplayErrorMessage(t, err, 'login_failed')
      setLoginError(message === t('login_failed') ? message : `${t('login_failed')}: ${message}`)
    } finally {
      setLoginLoading(false)
    }
  }

  function clearSession() {
    authStore.clearToken()
    setToken(null)
    setSelectedDevice(null)
  }

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
            aria-label={t('language')}
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

      <section className={`workspace ${token ? 'workspace-authenticated' : ''}`} aria-label={t('devices')}>
        {!token ? (
          <form className="login-panel" onSubmit={handleLogin}>
            <div className="panel-title">
              <LockKeyhole aria-hidden="true" size={18} />
              <span>{t('login')}</span>
            </div>
            <label>
              {t('email')}
              <input
                type="email"
                autoComplete="email"
                placeholder={t('email_placeholder')}
                value={email}
                onChange={(event) => setEmail(event.target.value)}
                required
              />
            </label>
            <label>
              {t('password')}
              <input
                type="password"
                autoComplete="current-password"
                placeholder={t('password_placeholder')}
                value={password}
                onChange={(event) => setPassword(event.target.value)}
                required
              />
            </label>
            {loginError ? (
              <p className="state-message state-message-error" role="alert">
                {loginError}
              </p>
            ) : null}
            <button type="submit" disabled={loginLoading}>
              <PlugZap aria-hidden="true" size={16} />
              {loginLoading ? t('connecting') : t('login')}
            </button>
          </form>
        ) : selectedDevice ? (
          <DeviceConsolePage
            device={selectedDevice}
            connectionsApi={connectionsApi}
            autoConnect
            t={t}
            onBack={() => setSelectedDevice(null)}
          />
        ) : (
          <DeviceListPage
            devicesApi={devicesApi}
            t={t}
            onSelectDevice={setSelectedDevice}
            onUnauthorized={clearSession}
          />
        )}
      </section>
    </main>
  )
}
