import { listen } from '@tauri-apps/api/event'
import { getActiveLanguage, getLocalApiUrl } from './api'
import { createEventCenterRuntime, type EventSourceLike } from './eventCenterRuntime'
import { renderEventCenter, setEventCenterItemExpanded } from './eventCenterView'
import { renderEventCenterWindowShell } from './eventCenterWindowView'
import {
  detectInitialLanguage,
  languageStorageKey,
  normalizeLanguage,
  translations,
  type LanguageCode
} from './i18n'
import './styles.css'

const languageChangedEvent = 'niuma-language-changed'
const app = document.querySelector<HTMLDivElement>('#app')

if (!app) {
  throw new Error('Missing #app root element')
}

const appRoot = app
let currentLanguage: LanguageCode = detectInitialLanguage()

const eventCenterRuntime = createEventCenterRuntime({
  getLocalApiUrl,
  createEventSource: (url) => new EventSource(url) as EventSourceLike,
  isActive: () => true,
  onChange: renderCurrentEventCenter,
  disconnectedText: () => translations[currentLanguage].eventCenterDisconnected
})

function renderShell() {
  document.documentElement.lang = currentLanguage
  appRoot.innerHTML = renderEventCenterWindowShell(currentLanguage)
  renderCurrentEventCenter()
}

function renderCurrentEventCenter() {
  const snapshot = eventCenterRuntime.snapshot()
  renderEventCenter({
    element: document.querySelector<HTMLElement>('#event-center-root'),
    language: currentLanguage,
    events: snapshot.events,
    expandedEventIds: snapshot.expandedEventIds,
    connected: snapshot.connected,
    connecting: snapshot.connecting,
    errorText: snapshot.errorText
  })
}

async function syncLanguageFromRuntime() {
  const nextLanguage = normalizeLanguage(await getActiveLanguage())
  if (nextLanguage === currentLanguage) {
    return
  }
  currentLanguage = nextLanguage
  window.localStorage.setItem(languageStorageKey, nextLanguage)
  renderShell()
}

appRoot.addEventListener('click', (event) => {
  const target = event.target instanceof HTMLElement ? event.target : null
  const eventId = target?.closest<HTMLElement>('[data-event-center-toggle]')?.dataset.eventCenterToggle
  if (!eventId) {
    return
  }
  const expanded = eventCenterRuntime.toggle(eventId)
  setEventCenterItemExpanded(document.querySelector<HTMLElement>('#event-center-root'), eventId, expanded)
})

listen<string>(languageChangedEvent, () => {
  void syncLanguageFromRuntime()
}).catch((error) => {
  const message = error instanceof Error ? error.message : String(error)
  console.error(`NiumaNotifier language sync failed: ${message}`)
})

window.addEventListener(languageChangedEvent, () => {
  void syncLanguageFromRuntime()
})

window.addEventListener('beforeunload', () => {
  eventCenterRuntime.stop()
})

renderShell()
eventCenterRuntime.start()
