import { messages, type SupportedLanguage, supportedLanguages } from './messages.js'

export function detectLanguage(language = navigator.language): SupportedLanguage {
  if (supportedLanguages.includes(language as SupportedLanguage)) return language as SupportedLanguage
  const base = language.split('-')[0]
  if (base === 'zh') return 'zh-CN'
  if (supportedLanguages.includes(base as SupportedLanguage)) return base as SupportedLanguage
  return 'en'
}

export function createTranslator(language = detectLanguage()) {
  const table = messages[language]
  return (key: string) => table[key] ?? messages.en[key] ?? key
}
