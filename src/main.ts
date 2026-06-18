import {
  dismissActiveBlocker,
  getActiveLanguage,
  getListenerConfig,
  getNotificationConfig,
  getLocalApiUrl,
  refreshMainState,
  saveListenerConfig,
  saveLanguagePreference,
  saveNotificationConfig,
  sendTestNotification,
  type ListenerToolConfig,
  type MainStatePayload,
  type NotificationChannel,
  type NotificationChannelConfig
} from './api'
import { listen } from '@tauri-apps/api/event'
import {
  detectInitialLanguage,
  languageStorageKey,
  normalizeLanguage,
  supportedLanguages,
  translations,
  type LanguageCode
} from './i18n'
import { renderDashboardShell } from './dashboardLayout'
import {
  collectNotificationChannels,
  formatNotificationTestResult,
  renderNotificationResult,
  renderNotificationPage as renderNotificationPageView
} from './notificationView'
import {
  isBlockingStatus,
  renderListenerTools,
  renderRequestDetail,
  renderStatusSummary
} from './statusView'
import { formatLocalTime } from './viewUtils'
import './styles.css'

const languageChangedEvent = 'niuma-language-changed'

const app = document.querySelector<HTMLDivElement>('#app')

if (!app) {
  throw new Error('Missing #app root element')
}

let currentLanguage = detectInitialLanguage()
let latestMainState: MainStatePayload | null = null
let fallbackTimer: number | undefined
let stream: EventSource | undefined
let clearBlockerConfirmTimer: number | undefined
let clearBlockerNeedsConfirm = false
let notificationChannels: NotificationChannelConfig[] = []
let notificationResultText = ''
let notificationBusyChannel: NotificationChannel | null = null
let notificationConfigLoaded = false
let notificationAutoSaveTimer: number | undefined
let notificationAutoSaveVersion = 0
let listenerTools: ListenerToolConfig[] = []
let listenerConfigLoaded = false
let listenerBusyToolId: string | null = null
let localApiUrlText = ''
let localSseConnected = false

app.innerHTML = renderDashboardShell()

const subtitleEl = document.querySelector<HTMLElement>('#subtitle')
const languageLabelEl = document.querySelector<HTMLElement>('#language-label')
const languageSelectEl = document.querySelector<HTMLSelectElement>('#language-select')
const currentStatusLabelEl = document.querySelector<HTMLElement>('#current-status-label')
const listenerHealthTitleEl = document.querySelector<HTMLElement>('#listener-health-title')
const localSseTitleEl = document.querySelector<HTMLElement>('#local-sse-title')
const localSseStateLabelEl = document.querySelector<HTMLElement>('#local-sse-state-label')
const localSseStateEl = document.querySelector<HTMLElement>('#local-sse-state')
const localSsePortLabelEl = document.querySelector<HTMLElement>('#local-sse-port-label')
const localSsePortEl = document.querySelector<HTMLElement>('#local-sse-port')
const localSsePathLabelEl = document.querySelector<HTMLElement>('#local-sse-path-label')
const localSsePathEl = document.querySelector<HTMLElement>('#local-sse-path')
const localSseUrlLabelEl = document.querySelector<HTMLElement>('#local-sse-url-label')
const localSseUrlEl = document.querySelector<HTMLElement>('#local-sse-url')
const notificationSettingsTitleEl = document.querySelector<HTMLElement>('#notification-settings-title')
const notificationFormEl = document.querySelector<HTMLElement>('#notification-form')
const notificationTestButton = document.querySelector<HTMLButtonElement>('#notification-test')
const statusSummaryEl = document.querySelector<HTMLElement>('#status-summary')
const updatedEl = document.querySelector<HTMLElement>('#updated')
const toolListenerListEl = document.querySelector<HTMLElement>('#tool-listener-list')
const codexListenerDescriptionEl = document.querySelector<HTMLElement>(
  '#codex-listener-description'
)
const requestDetailEl = document.querySelector<HTMLDListElement>('#request-detail')
const refreshButton = document.querySelector<HTMLButtonElement>('#refresh')
const clearBlockerButton = document.querySelector<HTMLButtonElement>('#clear-blocker')

setupLanguageSelect()
applyLanguage()
setupTrayLanguageSync()
syncLanguageFromRuntime()

async function refreshDashboard() {
  latestMainState = await refreshMainState()
  renderDashboard()
}

async function refreshListenerConfig() {
  const config = await getListenerConfig()
  listenerTools = normalizeListenerTools(config)
  listenerConfigLoaded = true
  renderToolListeners()
}

function renderDashboard() {
  if (latestMainState) {
    updatedEl!.textContent = formatLocalTime(latestMainState.updated_at, currentLanguage)
    clearBlockerButton!.hidden = !isBlockingStatus(latestMainState.status)
    renderStatusSummary({
      element: statusSummaryEl,
      state: latestMainState,
      language: currentLanguage
    })
    renderRequestDetail({
      element: requestDetailEl,
      state: latestMainState,
      language: currentLanguage
    })
  } else {
    updatedEl!.textContent = '-'
    clearBlockerButton!.hidden = true
    renderStatusSummary({
      element: statusSummaryEl,
      state: null,
      language: currentLanguage
    })
    renderRequestDetail({
      element: requestDetailEl,
      state: null,
      language: currentLanguage
    })
  }
  renderToolListeners()
  renderLocalSseStatus()
}

function renderToolListeners() {
  renderListenerTools({
    element: toolListenerListEl,
    tools: listenerTools,
    language: currentLanguage,
    busyToolId: listenerBusyToolId,
    loaded: listenerConfigLoaded
  })
}

function normalizeListenerTools(config: {
  codex_listening_enabled: boolean
  tools?: ListenerToolConfig[]
}) {
  if (config.tools && config.tools.length > 0) {
    return config.tools
  }
  return [
    {
      id: 'codex',
      plugin_id: 'builtin-codex',
      display_name: 'Codex',
      enabled: config.codex_listening_enabled,
      source: 'builtin',
      icon_url: null
    }
  ]
}

function renderNotificationPage() {
  renderNotificationPageView({
    formElement: notificationFormEl,
    settingsTitleElement: notificationSettingsTitleEl,
    language: currentLanguage,
    channels: notificationChannels,
    resultText: notificationResultText,
    busyChannel: notificationBusyChannel
  })
}

function syncNotificationDraftFromDom() {
  if (!notificationFormEl?.querySelector('.notification-channel')) {
    return
  }
  notificationChannels = collectNotificationChannels(notificationFormEl)
}

async function refreshNotificationConfig() {
  const data = await getNotificationConfig()
  notificationChannels = data.channels
  notificationConfigLoaded = true
}

function renderNotificationSettings(options: { syncDraft?: boolean } = {}) {
  if (options.syncDraft ?? true) {
    syncNotificationDraftFromDom()
  }
  renderNotificationPage()
}

function setupLanguageSelect() {
  if (!languageSelectEl) {
    return
  }
  languageSelectEl.innerHTML = supportedLanguages
    .map((code) => `<option value="${code}">${translations[code].languageName}</option>`)
    .join('')
  languageSelectEl.value = currentLanguage
  languageSelectEl.addEventListener('change', () => {
    const nextLanguage = normalizeLanguage(languageSelectEl.value)
    currentLanguage = nextLanguage
    window.localStorage.setItem(languageStorageKey, nextLanguage)
    applyLanguage()
    saveLanguagePreference(nextLanguage)
      .then((savedLanguage) => {
        const normalizedSavedLanguage = normalizeLanguage(savedLanguage)
        if (normalizedSavedLanguage !== currentLanguage) {
          currentLanguage = normalizedSavedLanguage
          window.localStorage.setItem(languageStorageKey, normalizedSavedLanguage)
          applyLanguage()
        }
      })
      .catch((error) => {
        updatedEl!.textContent = error instanceof Error ? error.message : String(error)
      })
  })
}

function setupTrayLanguageSync() {
  listen<string>(languageChangedEvent, () => {
    void syncLanguageFromRuntime()
  }).catch((error) => {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  })
  window.addEventListener(languageChangedEvent, () => {
    void syncLanguageFromRuntime()
  })
}

async function syncLanguageFromRuntime() {
  try {
    const nextLanguage = normalizeLanguage(await getActiveLanguage())
    if (nextLanguage === currentLanguage) {
      return
    }
    currentLanguage = nextLanguage
    window.localStorage.setItem(languageStorageKey, nextLanguage)
    applyLanguage()
  } catch (error) {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  }
}

function applyLanguage() {
  const t = translations[currentLanguage]
  document.documentElement.lang = currentLanguage
  subtitleEl!.textContent = t.appSubtitle
  if (languageLabelEl) {
    languageLabelEl.textContent = t.language
  }
  if (refreshButton) {
    refreshButton.textContent = t.refresh
  }
  if (notificationTestButton) {
    notificationTestButton.textContent = t.testSend
  }
  clearBlockerButton!.textContent = clearBlockerNeedsConfirm
    ? t.clearBlockerConfirmAgain
    : t.clearBlocker
  clearBlockerButton!.title = t.clearBlockerConfirm
  currentStatusLabelEl!.textContent = t.mainStatus
  listenerHealthTitleEl!.textContent = t.listenerStatus
  localSseTitleEl!.textContent = t.localSseInterface
  localSseStateLabelEl!.textContent = t.localSseState
  localSsePortLabelEl!.textContent = t.localSsePort
  localSsePathLabelEl!.textContent = t.localSsePath
  localSseUrlLabelEl!.textContent = t.localSseUrl
  codexListenerDescriptionEl!.textContent = t.toolListenerDescription
  renderToolListeners()
  if (languageSelectEl) {
    languageSelectEl.value = currentLanguage
    languageSelectEl.setAttribute('aria-label', t.language)
  }
  renderNotificationSettings()
  renderLocalSseStatus()
  renderDashboard()
}

function renderStatePayload(payload: MainStatePayload) {
  latestMainState = payload
  renderDashboard()
}

refreshButton?.addEventListener('click', () => {
  Promise.all([refreshDashboard(), refreshListenerConfig(), refreshNotificationConfig()])
    .then(() => renderNotificationSettings({ syncDraft: false }))
    .catch((error) => {
      updatedEl!.textContent = error instanceof Error ? error.message : String(error)
    })
})

toolListenerListEl?.addEventListener('change', async (event) => {
  const toggle = event.target instanceof HTMLInputElement ? event.target : null
  const toolId = toggle?.dataset.toolToggle
  if (!toggle || !toolId) {
    return
  }
  const nextEnabled = toggle.checked
  const previousTools = listenerTools.map((tool) => ({ ...tool }))
  listenerTools = listenerTools.map((tool) =>
    tool.id === toolId ? { ...tool, enabled: nextEnabled } : tool
  )
  listenerBusyToolId = toolId
  renderToolListeners()
  try {
    const saved = await saveListenerConfig({
      codex_listening_enabled:
        listenerTools.find((tool) => tool.id === 'codex')?.enabled ?? false,
      tool_listening_enabled: Object.fromEntries(
        listenerTools.map((tool) => [tool.id, tool.enabled])
      )
    })
    listenerTools = normalizeListenerTools(saved)
    listenerConfigLoaded = true
    await refreshDashboard()
  } catch (error) {
    listenerTools = previousTools
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  } finally {
    listenerBusyToolId = null
    renderToolListeners()
  }
})

function updateNotificationResult(text: string) {
  notificationResultText = text
  renderNotificationResult(notificationFormEl, currentLanguage, notificationResultText)
}

function scheduleNotificationAutoSave() {
  window.clearTimeout(notificationAutoSaveTimer)
  notificationAutoSaveTimer = window.setTimeout(() => {
    void saveNotificationDraft()
  }, 500)
}

async function saveNotificationDraft(options: { showResult?: boolean } = {}) {
  if (!notificationConfigLoaded || !notificationFormEl) {
    return
  }
  const t = translations[currentLanguage]
  const saveVersion = ++notificationAutoSaveVersion
  notificationChannels = collectNotificationChannels(notificationFormEl)
  try {
    await saveNotificationConfig(notificationChannels)
    if (saveVersion === notificationAutoSaveVersion || options.showResult) {
      updateNotificationResult(t.saved)
    }
  } catch (error) {
    if (saveVersion === notificationAutoSaveVersion || options.showResult) {
      const message = error instanceof Error ? error.message : String(error)
      updateNotificationResult(`${t.error}: ${message}`)
    }
  }
}

notificationFormEl?.addEventListener('input', scheduleNotificationAutoSave)
notificationFormEl?.addEventListener('change', scheduleNotificationAutoSave)

notificationTestButton?.addEventListener('click', async () => {
  const t = translations[currentLanguage]
  if (notificationBusyChannel || !notificationFormEl) {
    return
  }
  window.clearTimeout(notificationAutoSaveTimer)
  notificationChannels = collectNotificationChannels(notificationFormEl)
  const enabledChannels = notificationChannels
    .filter((item) => item.enabled)
    .map((item) => item.channel)
  notificationBusyChannel = enabledChannels[0] ?? 'bark'
  notificationTestButton.disabled = true
  updateNotificationResult(t.sending)
  try {
    await saveNotificationConfig(notificationChannels)
    if (enabledChannels.length === 0) {
      updateNotificationResult(t.noChannelsEnabled)
      return
    }
    const sentChannels: NotificationChannel[] = []
    const failedChannels: { channel: NotificationChannel; message: string }[] = []
    for (const item of enabledChannels) {
      try {
        await sendTestNotification(item)
        sentChannels.push(item)
      } catch (error) {
        // 手动测试应尽量覆盖所有启用渠道，单个渠道失败不能阻断后续渠道。
        failedChannels.push({
          channel: item,
          message: error instanceof Error ? error.message : String(error)
        })
      }
    }
    await refreshNotificationConfig()
    updateNotificationResult(formatNotificationTestResult(currentLanguage, sentChannels, failedChannels))
  } catch (error) {
    updateNotificationResult(error instanceof Error ? error.message : String(error))
  } finally {
    notificationBusyChannel = null
    notificationTestButton.disabled = false
  }
})

clearBlockerButton?.addEventListener('click', async () => {
  const t = translations[currentLanguage]
  if (!clearBlockerNeedsConfirm) {
    clearBlockerNeedsConfirm = true
    clearBlockerButton.textContent = t.clearBlockerConfirmAgain
    clearBlockerButton.title = t.clearBlockerConfirm
    clearBlockerButton.classList.add('needs-confirm')
    window.clearTimeout(clearBlockerConfirmTimer)
    // Tauri WebView 中系统 confirm 反馈不稳定；二次点击确认让点击响应直接体现在按钮上。
    clearBlockerConfirmTimer = window.setTimeout(resetClearBlockerButton, 3_000)
    return
  }

  window.clearTimeout(clearBlockerConfirmTimer)
  clearBlockerNeedsConfirm = false
  clearBlockerButton.disabled = true
  clearBlockerButton.classList.remove('needs-confirm')
  clearBlockerButton.textContent = t.clearBlockerClearing
  try {
    const response = await dismissActiveBlocker()
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    await refreshDashboard()
  } catch (error) {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  } finally {
    clearBlockerButton.disabled = false
    resetClearBlockerButton()
  }
})

function resetClearBlockerButton() {
  const t = translations[currentLanguage]
  clearBlockerNeedsConfirm = false
  clearBlockerButton?.classList.remove('needs-confirm')
  if (clearBlockerButton && !clearBlockerButton.disabled) {
    clearBlockerButton.textContent = t.clearBlocker
    clearBlockerButton.title = t.clearBlockerConfirm
  }
}

function renderLocalSseStatus() {
  if (!localSseStateEl || !localSsePortEl || !localSsePathEl || !localSseUrlEl) {
    return
  }
  const t = translations[currentLanguage]
  localSseStateEl.textContent = localSseConnected ? t.localSseConnected : t.localSsePolling
  localSseStateEl.className = localSseConnected ? 'endpoint-state connected' : 'endpoint-state polling'
  localSsePortEl.textContent = localApiUrlText ? portFromUrl(localApiUrlText) : t.loading
  localSsePathEl.textContent = '/api/v1/stream'
  localSseUrlEl.innerHTML = localApiUrlText
    ? `<span class="endpoint-url">${localApiUrlText}/api/v1/stream</span>`
    : t.loading
}

function portFromUrl(url: string) {
  try {
    return new URL(url).port || '-'
  } catch {
    return '-'
  }
}

async function startStream() {
  try {
    const apiUrl = await getLocalApiUrl()
    localApiUrlText = apiUrl
    renderLocalSseStatus()
    stream = new EventSource(`${apiUrl}/api/v1/stream`)
    stream.onopen = () => {
      localSseConnected = true
      renderLocalSseStatus()
    }
    stream.addEventListener('state', (message) => {
      const payload = JSON.parse((message as MessageEvent<string>).data) as MainStatePayload
      renderStatePayload(payload)
      stopFallbackPolling()
    })
    stream.onerror = () => {
      localSseConnected = false
      renderLocalSseStatus()
      startFallbackPolling()
    }
  } catch {
    localSseConnected = false
    renderLocalSseStatus()
    startFallbackPolling()
  }
}

function startFallbackPolling() {
  if (fallbackTimer !== undefined) {
    return
  }
  fallbackTimer = window.setInterval(() => {
    refreshDashboard().catch((error) => {
      updatedEl!.textContent = error instanceof Error ? error.message : String(error)
    })
  }, 2_000)
}

function stopFallbackPolling() {
  if (fallbackTimer === undefined) {
    return
  }
  window.clearInterval(fallbackTimer)
  fallbackTimer = undefined
}

Promise.all([refreshDashboard(), refreshListenerConfig(), refreshNotificationConfig()])
  .then(() => {
    renderNotificationSettings({ syncDraft: false })
  })
  .catch((error) => {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  })

startStream()

window.addEventListener('beforeunload', () => {
  stream?.close()
})
