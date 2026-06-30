import { ConfigProvider, theme } from 'antd'
import { useEffect } from 'react'
import { LoginPage } from './auth/loginPage.js'
import { I18nProvider, useI18n } from './i18n/index.js'

function ClientShell() {
  const { language, t } = useI18n()

  useEffect(() => {
    // 让浏览器标题和文档语言跟随当前检测到的界面语言。
    document.title = t('app_title')
    document.documentElement.lang = language
  }, [language, t])

  return (
    <main className="client-app-shell">
      <LoginPage />
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
