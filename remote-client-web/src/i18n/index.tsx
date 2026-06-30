import { createContext, useContext, useMemo, useState, type ReactNode } from 'react'
import { messages, type MessageKey, type SupportedLanguage, supportedLanguages } from './messages.js'

const languageStorageKey = 'niuma_remote_client_language'

type I18nContextValue = {
  language: SupportedLanguage
  setLanguage: (language: SupportedLanguage) => void
  t: (key: MessageKey) => string
}

const I18nContext = createContext<I18nContextValue | null>(null)

export function detectLanguage(language?: string): SupportedLanguage {
  const detectedLanguage = language ?? (typeof navigator !== 'undefined' ? navigator.language : 'en')

  if (supportedLanguages.includes(detectedLanguage as SupportedLanguage)) {
    return detectedLanguage as SupportedLanguage
  }

  const normalized = detectedLanguage.toLowerCase()
  const parts = normalized.split('-')
  const [base, scriptOrRegion, region] = parts
  if (base === 'zh') {
    // 按 BCP 47 脚本和常见地区优先区分繁简中文。
    if (scriptOrRegion === 'hant') return 'zh-TW'
    if (scriptOrRegion === 'hans') return 'zh-CN'
    if (scriptOrRegion === 'tw' || scriptOrRegion === 'hk' || scriptOrRegion === 'mo') return 'zh-TW'
    if (region === 'tw' || region === 'hk' || region === 'mo') return 'zh-TW'
    return 'zh-CN'
  }

  if (supportedLanguages.includes(base as SupportedLanguage)) return base as SupportedLanguage
  return 'en'
}

function readStoredLanguage(): SupportedLanguage | null {
  if (typeof window === 'undefined' || !window.localStorage) return null
  const stored = window.localStorage.getItem(languageStorageKey)
  return supportedLanguages.includes(stored as SupportedLanguage) ? (stored as SupportedLanguage) : null
}

export function I18nProvider({ children }: { children: ReactNode }) {
  const [language, setLanguageState] = useState<SupportedLanguage>(() => readStoredLanguage() ?? detectLanguage())
  function setLanguage(nextLanguage: SupportedLanguage) {
    setLanguageState(nextLanguage)
    if (typeof window !== 'undefined' && window.localStorage) {
      window.localStorage.setItem(languageStorageKey, nextLanguage)
    }
  }

  const value = useMemo<I18nContextValue>(
    () => ({
      language,
      setLanguage,
      t: (key) => messages[language][key] ?? messages.en[key] ?? key
    }),
    [language]
  )

  return <I18nContext.Provider value={value}>{children}</I18nContext.Provider>
}

export function useI18n() {
  const value = useContext(I18nContext)
  if (!value) {
    throw new Error('useI18n must be used inside I18nProvider')
  }
  return value
}
