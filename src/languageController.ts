import { listen } from '@tauri-apps/api/event'
import { getActiveLanguage, saveLanguagePreference } from './api'
import {
  languageStorageKey,
  normalizeLanguage,
  supportedLanguages,
  translations,
  type LanguageCode
} from './i18n'

export interface LanguageControllerOptions {
  eventName: string
  selectElement: HTMLSelectElement | null
  getLanguage: () => LanguageCode
  setLanguage: (language: LanguageCode) => void
  renderLanguage: () => void
  reportError: (error: unknown) => void
}

export function createLanguageController(options: LanguageControllerOptions) {
  async function syncLanguageFromRuntime() {
    try {
      const nextLanguage = normalizeLanguage(await getActiveLanguage())
      if (nextLanguage === options.getLanguage()) {
        return
      }
      options.setLanguage(nextLanguage)
      window.localStorage.setItem(languageStorageKey, nextLanguage)
      options.renderLanguage()
    } catch (error) {
      options.reportError(error)
    }
  }

  return {
    setupSelect() {
      if (!options.selectElement) {
        return
      }
      options.selectElement.innerHTML = supportedLanguages
        .map((code) => `<option value="${code}">${translations[code].languageName}</option>`)
        .join('')
      options.selectElement.value = options.getLanguage()
      options.selectElement.addEventListener('change', () => {
        const nextLanguage = normalizeLanguage(options.selectElement!.value)
        options.setLanguage(nextLanguage)
        window.localStorage.setItem(languageStorageKey, nextLanguage)
        options.renderLanguage()
        saveLanguagePreference(nextLanguage)
          .then((savedLanguage) => {
            const normalizedSavedLanguage = normalizeLanguage(savedLanguage)
            if (normalizedSavedLanguage !== options.getLanguage()) {
              options.setLanguage(normalizedSavedLanguage)
              window.localStorage.setItem(languageStorageKey, normalizedSavedLanguage)
              options.renderLanguage()
            }
          })
          .catch(options.reportError)
      })
    },
    setupRuntimeSync() {
      listen<string>(options.eventName, () => {
        void syncLanguageFromRuntime()
      }).catch(options.reportError)
      window.addEventListener(options.eventName, () => {
        void syncLanguageFromRuntime()
      })
    },
    syncLanguageFromRuntime
  }
}
