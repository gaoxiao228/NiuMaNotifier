/// <reference types="vite/client" />

import { ConfigProvider, theme } from 'antd'
import { useCallback, useEffect, useMemo, useState } from 'react'
import { createAuthApi } from './api/authApi.js'
import { createConnectionsApi } from './api/connectionsApi.js'
import { createDevicesApi, type RemoteDevice } from './api/devicesApi.js'
import { createHttpClient } from './api/httpClient.js'
import { createAuthStore, type AuthSession } from './auth/authStore.js'
import { LoginPage } from './auth/loginPage.js'
import { DeviceListPage } from './devices/deviceListPage.js'
import { I18nProvider, useI18n } from './i18n/index.js'
import {
  createRemoteDeviceSessionController,
  type RemoteDeviceSessionSnapshot
} from './remote/remoteDeviceSessionController.js'
import { getStableClientId } from './remote/clientId.js'
import { SessionConsolePage } from './sessions/sessionConsolePage.js'
import { toDisplayErrorMessage } from './shared/errorMessage.js'

export function resolveRemoteServerUrl(): string {
  const envUrl = import.meta.env.VITE_REMOTE_SERVER_URL
  if (typeof envUrl === 'string' && envUrl.trim()) return envUrl.trim()
  return typeof window !== 'undefined' ? window.location.origin : ''
}

function ClientShell() {
  const { language, t } = useI18n()
  const authStore = useMemo(() => createAuthStore(), [])
  const [session, setSession] = useState<AuthSession>(() => authStore.getSnapshot())
  const [devices, setDevices] = useState<RemoteDevice[]>([])
  const [selectedDevice, setSelectedDevice] = useState<RemoteDevice | null>(null)
  const [sessionSnapshot, setSessionSnapshot] = useState<RemoteDeviceSessionSnapshot | null>(null)
  const [loginLoading, setLoginLoading] = useState(false)
  const [devicesLoading, setDevicesLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    // 让浏览器标题和文档语言跟随当前检测到的界面语言。
    document.title = t('app_title')
    document.documentElement.lang = language
  }, [language, t])

  useEffect(() => {
    return authStore.subscribe((nextSession) => {
      setSession(nextSession)
      if (!nextSession.accessToken) {
        setDevices([])
        setSelectedDevice(null)
      }
    })
  }, [authStore])

  const http = useMemo(
    () =>
      createHttpClient({
        baseUrl: resolveRemoteServerUrl(),
        getAccessToken: () => authStore.getSnapshot().accessToken,
        onAuthExpired: () => {
          authStore.clear()
          setError(t('auth_expired_message'))
        }
      }),
    [authStore, t]
  )
  const authApi = useMemo(() => createAuthApi(http), [http])
  const devicesApi = useMemo(() => createDevicesApi(http), [http])
  const connectionsApi = useMemo(() => createConnectionsApi(http), [http])

  const loadDevices = useCallback(async () => {
    setDevicesLoading(true)
    setError(null)
    try {
      const response = await devicesApi.list()
      setDevices(response.list)
    } catch (cause) {
      // 鉴权过期会由 HTTP client 清空 session；此处只处理仍处于登录态的普通列表错误。
      if (authStore.getSnapshot().accessToken) {
        setError(toDisplayErrorMessage(t, cause, 'devices_load_error'))
      }
    } finally {
      setDevicesLoading(false)
    }
  }, [authStore, devicesApi, t])

  useEffect(() => {
    if (session.accessToken && !selectedDevice) {
      void loadDevices()
    }
  }, [loadDevices, selectedDevice, session.accessToken])

  useEffect(() => {
    if (!selectedDevice || !session.accessToken) {
      setSessionSnapshot(null)
      return
    }

    let closed = false
    const controller = createRemoteDeviceSessionController({
      device: selectedDevice,
      connectionsApi,
      clientId: getStableClientId(),
      onSnapshot: (snapshot) => {
        if (!closed) setSessionSnapshot(snapshot)
      }
    })

    void controller.connect().catch(() => {
      // controller 已将常规连接错误写入 snapshot；这里只兜底处理异常 reject。
      if (!closed) setSessionSnapshot((current) => current)
    })

    return () => {
      closed = true
      controller.close()
    }
  }, [connectionsApi, selectedDevice, session.accessToken])

  async function handleLogin(email: string, password: string) {
    setLoginLoading(true)
    setError(null)
    try {
      const response = await authApi.login(email, password)
      authStore.setSession({
        accessToken: response.access_token,
        refreshToken: response.refresh_token,
        user: response.user
      })
      setSelectedDevice(null)
    } catch (cause) {
      if (authStore.getSnapshot().accessToken) return
      setError(toDisplayErrorMessage(t, cause, 'login_error'))
    } finally {
      setLoginLoading(false)
    }
  }

  function handleLogout() {
    setSelectedDevice(null)
    setSessionSnapshot(null)
    authStore.clear()
    setError(null)
  }

  function handleBackToDevices() {
    setSelectedDevice(null)
    setSessionSnapshot(null)
  }

  if (!session.accessToken || !session.user) {
    return (
      <main className="client-app-shell">
        <LoginPage loading={loginLoading} error={error} onLogin={handleLogin} />
      </main>
    )
  }

  if (selectedDevice) {
    return (
      <main className="client-app-shell app-wide">
        <SessionConsolePage
          device={selectedDevice}
          snapshot={sessionSnapshot}
          onBack={handleBackToDevices}
          onLogout={handleLogout}
        />
      </main>
    )
  }

  return (
    <main className="client-app-shell app-wide">
      <DeviceListPage
        devices={devices}
        loading={devicesLoading}
        error={error}
        userEmail={session.user.email}
        onRefresh={loadDevices}
        onLogout={handleLogout}
        onOpenDevice={(device) => setSelectedDevice(device)}
      />
    </main>
  )
}

export function App() {
  return (
    <ConfigProvider
      theme={{
        algorithm: theme.defaultAlgorithm,
        token: {
          borderRadius: 6,
          colorPrimary: '#1677ff'
        }
      }}
    >
      <I18nProvider>
        <ClientShell />
      </I18nProvider>
    </ConfigProvider>
  )
}
