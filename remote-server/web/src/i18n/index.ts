import { messages, type SupportedLanguage, supportedLanguages } from './messages.js'

export function detectLanguage(language?: string): SupportedLanguage {
  const detectedLanguage = language ?? (typeof navigator !== 'undefined' ? navigator.language : 'en')

  if (supportedLanguages.includes(detectedLanguage as SupportedLanguage)) {
    return detectedLanguage as SupportedLanguage
  }

  const normalized = detectedLanguage.toLowerCase()
  const base = normalized.split('-')[0]
  if (base === 'zh') {
    // 港澳台区域默认使用繁体，其余中文区域使用简体。
    if (normalized === 'zh-tw' || normalized === 'zh-hk' || normalized === 'zh-mo') return 'zh-TW'
    return 'zh-CN'
  }
  if (supportedLanguages.includes(base as SupportedLanguage)) return base as SupportedLanguage
  return 'en'
}

export function createTranslator(language = detectLanguage()) {
  const table = messages[language]
  return (key: string) => table[key] ?? messages.en[key] ?? key
}
