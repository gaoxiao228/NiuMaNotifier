import { messages, type SupportedLanguage, supportedLanguages } from './messages.js'

export function detectLanguage(language?: string): SupportedLanguage {
  const detectedLanguage = language ?? (typeof navigator !== 'undefined' ? navigator.language : 'en')

  if (supportedLanguages.includes(detectedLanguage as SupportedLanguage)) {
    return detectedLanguage as SupportedLanguage
  }

  const normalized = detectedLanguage.toLowerCase()
  const parts = normalized.split('-')
  const [base, scriptOrRegion, region] = parts
  if (base === 'zh') {
    // 优先按 BCP 47 脚本子标签判断繁简，再按港澳台区域回退到繁体。
    if (scriptOrRegion === 'hant') return 'zh-TW'
    if (scriptOrRegion === 'hans') return 'zh-CN'
    if (scriptOrRegion === 'tw' || scriptOrRegion === 'hk' || scriptOrRegion === 'mo') return 'zh-TW'
    if (region === 'tw' || region === 'hk' || region === 'mo') return 'zh-TW'
    return 'zh-CN'
  }
  if (supportedLanguages.includes(base as SupportedLanguage)) return base as SupportedLanguage
  return 'en'
}

export function createTranslator(language = detectLanguage()) {
  const table = messages[language]
  return (key: string) => table[key] ?? messages.en[key] ?? key
}
