import {
  dismissActiveBlocker,
  getActiveLanguage,
  getListenerConfig,
  getNotificationRecords,
  getPluginConfig,
  getPlugins,
  getLocalApiUrl,
  refreshMainState,
  removePlugin,
  setPluginEnabled,
  savePluginConfig,
  saveListenerConfig,
  saveLanguagePreference,
  selectAndImportPluginDir,
  sendTestNotification,
  type ListenerToolConfig,
  type MainStatePayload,
  type NotificationRecord,
  type PluginManagementItem,
  type PluginRuntimeStatus
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
  formatNotificationTestResult,
  renderNotificationHistoryOnly,
  renderNotificationResult,
  renderNotificationPage as renderNotificationPageView
} from './notificationView'
import {
  isBlockingStatus,
  renderListenerTools,
  renderRequestDetail,
  renderStatusSummary
} from './statusView'
import {
  renderPluginImportResult,
  renderPluginManagement,
  renderSettingsShell,
  type SettingsPanel
} from './settingsView'
import { renderEventCenter, setEventCenterItemExpanded } from './eventCenterView'
import { createEventCenterRuntime, type EventSourceLike } from './eventCenterRuntime'
import {
  hasPluginReachedTransitionTarget,
  isPluginTransitioning,
  mergePendingPluginTransition,
  type PluginTransitionTarget
} from './pluginTransition'
import {
  createPluginRuntimeRefresh,
  type PluginRuntimeRefreshController
} from './pluginRuntimeRefresh'
import { formatLocalTime } from './viewUtils'
import './styles.css'

const languageChangedEvent = 'niuma-language-changed'
const pluginTransitionPollDelayMs = 500
const pluginTransitionPollMaxAttempts = 20
const pluginRuntimeRefreshIntervalMs = 3_000
const stateStreamPath = '/api/v1/state/stream'

const app = document.querySelector<HTMLDivElement>('#app')

if (!app) {
  throw new Error('Missing #app root element')
}

let currentLanguage = detectInitialLanguage()
let latestMainState: MainStatePayload | null = null
let fallbackTimer: number | undefined
let stream: EventSource | undefined
let pluginRuntimeRefresh: PluginRuntimeRefreshController | undefined
let clearBlockerConfirmTimer: number | undefined
let clearBlockerNeedsConfirm = false
let notificationResultText = ''
let listenerTools: ListenerToolConfig[] = []
let listenerConfigLoaded = false
let listenerBusyToolId: string | null = null
let plugins: PluginManagementItem[] = []
let pluginConfigs: Record<string, Record<string, unknown>> = {}
let pluginBusyId: string | null = null
let pluginConfigBusyId: string | null = null
let pendingPluginTransition: PluginTransitionTarget | null = null
let pluginImportBusy = false
let pluginImportResultText = ''
let pluginConfigResultText = ''
let activeView: 'dashboard' | 'settings' = 'dashboard'
let activeSettingsPanel: SettingsPanel = 'plugins'
let notificationRecords: NotificationRecord[] = []
let notificationRecordsLoaded = false
let localApiUrlText = ''
let localSseConnected = false

const eventCenterRuntime = createEventCenterRuntime({
  getLocalApiUrl,
  createEventSource: (url) => new EventSource(url) as EventSourceLike,
  isActive: () => activeView === 'settings' && activeSettingsPanel === 'event-center',
  onChange: renderSettingsEventCenter,
  disconnectedText: () => translations[currentLanguage].eventCenterDisconnected
})

app.innerHTML = renderDashboardShell()

const subtitleEl = document.querySelector<HTMLElement>('#subtitle')
const dashboardHeaderEl = document.querySelector<HTMLElement>('#dashboard-header')
const dashboardViewEl = document.querySelector<HTMLElement>('#dashboard-view')
const settingsViewEl = document.querySelector<HTMLElement>('#settings-view')
const settingsShellEl = document.querySelector<HTMLElement>('#settings-shell')
const settingsOpenButton = document.querySelector<HTMLButtonElement>('#settings-open')
const settingsBackButton = document.querySelector<HTMLButtonElement>('#settings-back')
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

async function refreshPlugins() {
  const data = await getPlugins()
  applyPluginSnapshot(data.list)
  await refreshPluginConfigs(data.list)
  renderPluginSettings()
  renderNotificationSettings()
}

async function refreshPluginRuntimeSnapshot() {
  const data = await getPlugins()
  applyPluginSnapshot(data.list)
  renderPluginSettings()
  renderNotificationSettings()
}

function startPluginRuntimeRefresh() {
  if (!pluginRuntimeRefresh) {
    pluginRuntimeRefresh = createPluginRuntimeRefresh({
      intervalMs: pluginRuntimeRefreshIntervalMs,
      refresh: refreshPluginRuntimeSnapshot
    })
  }
  // 插件运行态保存在内存中，主状态 SSE 不一定变化；用轻量轮询同步界面显示。
  pluginRuntimeRefresh.start()
}

function applyPluginSnapshot(snapshot: PluginManagementItem[]) {
  plugins = snapshot.map((plugin) => mergePendingPluginTransition(plugin, pendingPluginTransition))
}

async function refreshPluginConfigs(snapshot: PluginManagementItem[] = plugins) {
  const configurablePlugins = snapshot.filter((plugin) => plugin.config_schema.length > 0)
  const entries = await Promise.all(
    configurablePlugins.map(async (plugin) => {
      const payload = await getPluginConfig(plugin.id)
      return [plugin.id, payload.config] as const
    })
  )
  pluginConfigs = Object.fromEntries(entries)
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

function renderActiveView() {
  if (dashboardHeaderEl) {
    dashboardHeaderEl.hidden = activeView !== 'dashboard'
  }
  if (dashboardViewEl) {
    dashboardViewEl.hidden = activeView !== 'dashboard'
  }
  if (settingsViewEl) {
    settingsViewEl.hidden = activeView !== 'settings'
  }
}

function showDashboardView() {
  activeView = 'dashboard'
  stopEventCenterStream()
  renderActiveView()
}

function showSettingsView() {
  activeView = 'settings'
  renderActiveView()
  renderSettings()
  refreshPlugins().catch((error) => {
    pluginImportResultText = error instanceof Error ? error.message : String(error)
    renderPluginSettings()
  })
  // 通知历史只属于通知历史侧边栏面板，避免在插件管理页触发无关刷新。
  if (activeSettingsPanel === 'notification-history' && !notificationRecordsLoaded) {
    void refreshNotificationRecords()
  }
  if (activeSettingsPanel === 'event-center') {
    startEventCenterStream()
  }
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

function renderSettings() {
  if (!settingsShellEl) {
    return
  }
  settingsShellEl.innerHTML = renderSettingsShell({
    language: currentLanguage,
    activePanel: activeSettingsPanel
  })
  renderPluginSettings()
  renderSettingsNotificationHistory()
  renderSettingsEventCenter()
}

function renderPluginSettings() {
  renderPluginManagement({
    element: document.querySelector<HTMLElement>('#plugin-management-list'),
    language: currentLanguage,
    plugins,
    busyPluginId: pluginBusyId,
    busyConfigPluginId: pluginConfigBusyId,
    importBusy: pluginImportBusy,
    resultText: pluginImportResultText,
    configResultText: pluginConfigResultText,
    pluginConfigs
  })
  const importButton = document.querySelector<HTMLButtonElement>('#plugin-import')
  if (importButton) {
    const t = translations[currentLanguage]
    importButton.textContent = pluginImportBusy ? t.importingPlugin : t.importPlugin
    importButton.disabled = pluginImportBusy
  }
  renderPluginImportResult(
    document.querySelector<HTMLElement>('#plugin-import-result'),
    currentLanguage,
    pluginImportResultText
  )
}

async function refreshNotificationRecords() {
  notificationRecordsLoaded = false
  renderSettingsNotificationHistory()
  try {
    const data = await getNotificationRecords()
    notificationRecords = data.list
  } catch (error) {
    notificationRecords = [
      {
        id: 'settings-notification-history-error',
        event_id: 'settings-notification-history-error',
        event_type: 'unknown',
        channel: translations[currentLanguage].error,
        status: 'failed',
        title: translations[currentLanguage].error,
        body: null,
        reason: 'unknown',
        error_message: error instanceof Error ? error.message : String(error),
        created_at: new Date().toISOString(),
        sent_at: null
      }
    ]
  } finally {
    notificationRecordsLoaded = true
    renderSettingsNotificationHistory()
  }
}

function renderSettingsNotificationHistory() {
  // 通知历史使用独立列表渲染，避免影响旧通知设置表单的草稿内容。
  renderNotificationHistoryOnly({
    historyElement: document.querySelector<HTMLOListElement>('#settings-notification-history'),
    language: currentLanguage,
    records: notificationRecords,
    recordsLoaded: notificationRecordsLoaded
  })
}

function renderSettingsEventCenter() {
  const snapshot = eventCenterRuntime.snapshot()
  const element = document.querySelector<HTMLElement>('#settings-event-center')
  // 展开详情必须保持当前事件行位置稳定，只恢复列表滚动位置，不主动滚动到详情。
  const previousScrollTop = element?.querySelector<HTMLOListElement>('.event-center-list')?.scrollTop ?? 0
  renderEventCenter({
    element,
    language: currentLanguage,
    events: snapshot.events,
    expandedEventIds: snapshot.expandedEventIds,
    connected: snapshot.connected,
    connecting: snapshot.connecting,
    errorText: snapshot.errorText
  })
  const nextList = element?.querySelector<HTMLOListElement>('.event-center-list')
  if (nextList) {
    nextList.scrollTop = previousScrollTop
  }
}

function startEventCenterStream() {
  eventCenterRuntime.start()
}

function stopEventCenterStream() {
  eventCenterRuntime.stop()
}

function renderNotificationPage() {
  renderNotificationPageView({
    formElement: notificationFormEl,
    settingsTitleElement: notificationSettingsTitleEl,
    language: currentLanguage,
    notificationPlugins: notificationPlugins(),
    resultText: notificationResultText,
    busyPluginId: pluginBusyId
  })
}

function notificationPlugins() {
  return plugins.filter((plugin) => plugin.kind === 'notification')
}

function enabledNotificationTestPlugins() {
  return plugins.filter(
    (plugin) =>
      plugin.kind === 'notification' &&
      plugin.enabled &&
      plugin.capabilities.includes('notification_test')
  )
}

function renderNotificationSettings() {
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
  settingsOpenButton!.setAttribute('aria-label', t.settingsButton)
  settingsOpenButton!.title = t.settingsButton
  settingsBackButton!.textContent = t.backToDashboard
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
  renderSettings()
  if (activeSettingsPanel === 'event-center') {
    renderSettingsEventCenter()
  }
  renderDashboard()
  renderActiveView()
}

function renderStatePayload(payload: MainStatePayload) {
  latestMainState = payload
  renderDashboard()
}

refreshButton?.addEventListener('click', () => {
  Promise.all([refreshDashboard(), refreshListenerConfig(), refreshPlugins()])
    .then(() => renderNotificationSettings())
    .catch((error) => {
      updatedEl!.textContent = error instanceof Error ? error.message : String(error)
    })
})

settingsOpenButton?.addEventListener('click', showSettingsView)
settingsBackButton?.addEventListener('click', showDashboardView)

settingsViewEl?.addEventListener('click', async (event) => {
  const target = event.target instanceof HTMLElement ? event.target : null
  const t = translations[currentLanguage]
  const settingsPanel = target?.dataset.settingsPanel
  if (
    settingsPanel === 'plugins' ||
    settingsPanel === 'event-center' ||
    settingsPanel === 'notification-history'
  ) {
    if (settingsPanel === activeSettingsPanel) {
      return
    }
    stopEventCenterStream()
    activeSettingsPanel = settingsPanel
    renderSettings()
    if (activeSettingsPanel === 'notification-history' && !notificationRecordsLoaded) {
      await refreshNotificationRecords()
    }
    if (activeSettingsPanel === 'event-center') {
      startEventCenterStream()
    }
    return
  }
  if (target?.id === 'settings-notification-history-refresh') {
    await refreshNotificationRecords()
    return
  }
  const eventCenterToggleId = target
    ?.closest<HTMLElement>('[data-event-center-toggle]')
    ?.dataset.eventCenterToggle
  if (eventCenterToggleId) {
    const expanded = eventCenterRuntime.toggle(eventCenterToggleId)
    setEventCenterItemExpanded(
      document.querySelector<HTMLElement>('#settings-event-center'),
      eventCenterToggleId,
      expanded
    )
    return
  }
  const pluginConfigSaveId = target?.dataset.pluginConfigSave
  if (pluginConfigSaveId) {
    const form = document.querySelector<HTMLFormElement>(
      `.plugin-config-form[data-plugin-config-form="${cssEscape(pluginConfigSaveId)}"]`
    )
    if (!form) {
      return
    }
    pluginConfigBusyId = pluginConfigSaveId
    pluginConfigResultText = ''
    renderPluginSettings()
    try {
      const result = await savePluginConfig(pluginConfigSaveId, collectPluginConfig(form))
      pluginConfigs = {
        ...pluginConfigs,
        [pluginConfigSaveId]: result.config
      }
      pluginConfigResultText = t.saved
      await refreshPlugins()
    } catch (error) {
      pluginConfigResultText = error instanceof Error ? error.message : String(error)
    } finally {
      pluginConfigBusyId = null
      renderPluginSettings()
      renderNotificationSettings()
    }
    return
  }
  const pluginIdToRemove = target?.dataset.pluginRemove
  if (pluginIdToRemove) {
    if (isPluginTransitioning(plugins.find((plugin) => plugin.id === pluginIdToRemove))) {
      return
    }
    pluginBusyId = pluginIdToRemove
    pluginImportResultText = ''
    renderPluginSettings()
    try {
      const result = await removePlugin(pluginIdToRemove)
      applyPluginSnapshot(result.plugins)
      pluginImportResultText = t.pluginRemoveSuccess
      await Promise.all([refreshListenerConfig(), refreshDashboard()])
    } catch (error) {
      pluginImportResultText = error instanceof Error ? error.message : String(error)
    } finally {
      pluginBusyId = null
      renderPluginSettings()
      renderNotificationSettings()
    }
    return
  }
  if (target?.id !== 'plugin-import') {
    return
  }
  pluginImportBusy = true
  pluginImportResultText = ''
  renderPluginSettings()
  try {
    const result = await selectAndImportPluginDir()
    applyPluginSnapshot(result.plugins)
    pluginImportResultText = result.cancelled ? t.pluginImportCancelled : t.pluginImportSuccess
    await refreshListenerConfig()
  } catch (error) {
    pluginImportResultText = error instanceof Error ? error.message : String(error)
  } finally {
    pluginImportBusy = false
    renderPluginSettings()
    renderNotificationSettings()
  }
})

settingsViewEl?.addEventListener('change', async (event) => {
  const toggle = event.target instanceof HTMLInputElement ? event.target : null
  const pluginId = toggle?.dataset.pluginToggle
  if (!toggle || !pluginId) {
    return
  }
  const nextEnabled = toggle.checked
  const previousPlugins = plugins.map((plugin) => ({ ...plugin }))
  const selectedPlugin = plugins.find((plugin) => plugin.id === pluginId)
  // 内置和外部事件监听插件都由同一个插件运行管理器启动，切换时都需要展示过渡态。
  const tracksRuntimeTransition = Boolean(selectedPlugin)
  pendingPluginTransition = tracksRuntimeTransition
    ? {
        pluginId,
        desiredEnabled: nextEnabled,
        optimisticStatus: (nextEnabled ? 'starting' : 'stopping') as PluginRuntimeStatus
      }
    : null
  plugins = plugins.map((plugin) =>
    plugin.id === pluginId
      ? {
          ...plugin,
          enabled: nextEnabled,
          runtime_status: tracksRuntimeTransition
            ? ((nextEnabled ? 'starting' : 'stopping') as PluginRuntimeStatus)
            : plugin.runtime_status
        }
      : plugin
  )
  pluginBusyId = pluginId
  renderPluginSettings()
  renderNotificationSettings()
  try {
    const result = await setPluginEnabled(pluginId, nextEnabled)
    applyPluginSnapshot(result.plugins)
    if (tracksRuntimeTransition) {
      const reachedTarget = await waitForPluginTargetState(pluginId, nextEnabled)
      if (!reachedTarget) {
        pendingPluginTransition = null
        await refreshPlugins()
      }
    } else {
      await refreshPlugins()
    }
    await Promise.all([refreshListenerConfig(), refreshDashboard()])
  } catch (error) {
    pendingPluginTransition = null
    plugins = previousPlugins
    pluginImportResultText = error instanceof Error ? error.message : String(error)
  } finally {
    pendingPluginTransition = null
    pluginBusyId = null
    renderPluginSettings()
    renderNotificationSettings()
  }
})

settingsViewEl?.addEventListener('submit', (event) => {
  if (
    event.target instanceof HTMLFormElement &&
    event.target.classList.contains('plugin-config-form')
  ) {
    event.preventDefault()
  }
})

async function waitForPluginTargetState(pluginId: string, desiredEnabled: boolean) {
  for (let attempt = 0; attempt < pluginTransitionPollMaxAttempts; attempt += 1) {
    const data = await getPlugins()
    const plugin = data.list.find((item) => item.id === pluginId)
    applyPluginSnapshot(data.list)
    renderPluginSettings()
    renderNotificationSettings()
    if (hasPluginReachedTransitionTarget(plugin, desiredEnabled)) {
      return true
    }
    await delay(pluginTransitionPollDelayMs)
  }
  return false
}

function delay(ms: number) {
  return new Promise<void>((resolve) => window.setTimeout(resolve, ms))
}

function collectPluginConfig(form: HTMLFormElement) {
  const config: Record<string, unknown> = {}
  form.querySelectorAll<HTMLInputElement | HTMLSelectElement>('[data-plugin-config-field]').forEach(
    (input) => {
      const key = input.dataset.pluginConfigField
      if (!key) {
        return
      }
      if (input instanceof HTMLInputElement && input.type === 'checkbox') {
        config[key] = input.checked
      } else if (input instanceof HTMLInputElement && input.type === 'number') {
        config[key] = input.value === '' ? null : Number(input.value)
      } else {
        config[key] = input.value
      }
    }
  )
  return config
}

function cssEscape(value: string) {
  return typeof CSS !== 'undefined' && CSS.escape ? CSS.escape(value) : value.replace(/"/g, '\\"')
}

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

notificationFormEl?.addEventListener('change', async (event) => {
  const toggle = event.target instanceof HTMLInputElement ? event.target : null
  const pluginId = toggle?.dataset.notificationPluginToggle
  if (!toggle || !pluginId) {
    return
  }
  const nextEnabled = toggle.checked
  const previousPlugins = plugins.map((plugin) => ({ ...plugin }))
  const selectedPlugin = plugins.find((plugin) => plugin.id === pluginId)
  if (!selectedPlugin || selectedPlugin.kind !== 'notification') {
    return
  }
  pendingPluginTransition = {
    pluginId,
    desiredEnabled: nextEnabled,
    optimisticStatus: (nextEnabled ? 'starting' : 'stopping') as PluginRuntimeStatus
  }
  plugins = plugins.map((plugin) =>
    plugin.id === pluginId
      ? {
          ...plugin,
          enabled: nextEnabled,
          runtime_status: (nextEnabled ? 'starting' : 'stopping') as PluginRuntimeStatus
        }
      : plugin
  )
  pluginBusyId = pluginId
  renderNotificationSettings()
  renderPluginSettings()
  try {
    const result = await setPluginEnabled(pluginId, nextEnabled)
    applyPluginSnapshot(result.plugins)
    const reachedTarget = await waitForPluginTargetState(pluginId, nextEnabled)
    if (!reachedTarget) {
      pendingPluginTransition = null
      await refreshPlugins()
    }
    await Promise.all([refreshListenerConfig(), refreshDashboard()])
  } catch (error) {
    pendingPluginTransition = null
    plugins = previousPlugins
    updateNotificationResult(error instanceof Error ? error.message : String(error))
  } finally {
    pendingPluginTransition = null
    pluginBusyId = null
    renderNotificationSettings()
    renderPluginSettings()
  }
})

notificationTestButton?.addEventListener('click', async () => {
  const t = translations[currentLanguage]
  if (pluginBusyId) {
    return
  }
  notificationTestButton.disabled = true
  updateNotificationResult(t.sending)
  try {
    await refreshPlugins()
    const enabledPlugins = enabledNotificationTestPlugins()
    pluginBusyId = enabledPlugins[0]?.id ?? null
    renderNotificationSettings()
    if (enabledPlugins.length === 0) {
      updateNotificationResult(t.noChannelsEnabled)
      return
    }
    const sentPluginIds: string[] = []
    const failedPlugins: { pluginId: string; message: string }[] = []
    for (const item of enabledPlugins) {
      pluginBusyId = item.id
      renderNotificationSettings()
      try {
        await sendTestNotification(item.id)
        sentPluginIds.push(item.id)
      } catch (error) {
        // 手动测试应尽量覆盖所有启用插件，单个插件失败不能阻断后续插件。
        failedPlugins.push({
          pluginId: item.id,
          message: error instanceof Error ? error.message : String(error)
        })
      }
    }
    updateNotificationResult(formatNotificationTestResult(currentLanguage, sentPluginIds, failedPlugins))
  } catch (error) {
    updateNotificationResult(error instanceof Error ? error.message : String(error))
  } finally {
    pluginBusyId = null
    renderNotificationSettings()
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
  localSsePathEl.textContent = stateStreamPath
  localSseUrlEl.innerHTML = localApiUrlText
    ? `<span class="endpoint-url">${localApiUrlText}${stateStreamPath}</span>`
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
    stream = new EventSource(`${apiUrl}${stateStreamPath}`)
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

Promise.all([refreshDashboard(), refreshListenerConfig(), refreshPlugins()])
  .then(() => {
    renderNotificationSettings()
  })
  .catch((error) => {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  })

startStream()
startPluginRuntimeRefresh()

window.addEventListener('beforeunload', () => {
  stream?.close()
  pluginRuntimeRefresh?.stop()
  eventCenterRuntime.stop()
})
