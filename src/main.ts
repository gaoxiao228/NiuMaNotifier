import {
  dismissActiveBlocker,
  getListenerConfig,
  getNotificationRecords,
  getPluginConfig,
  getPlugins,
  getLocalApiUrl,
  refreshMainState,
  removePlugin,
  runPluginAction,
  setPluginEnabled,
  savePluginConfig,
  saveListenerConfig,
  selectAndImportPluginDir,
  submitInputAnswer,
  sendTestNotification,
  submitApprovalDecision,
  type EventInteractionQuestion,
  type ListenerToolConfig,
  type MainStatePayload,
  type NotificationRecord,
  type PluginManagementItem
} from './api'
import {
  detectInitialLanguage,
  translations,
  type LanguageCode
} from './i18n'
import { renderDashboardShell } from './dashboardLayout'
import {
  formatNotificationTestResult,
  renderNotificationHistoryOnly,
  renderNotificationPage as renderNotificationPageView
} from './notificationView'
import {
  blockerActionLabel,
  isBlockingStatus,
  renderListenerTools,
  renderRequestDetail,
  renderStatusSummary,
  shouldShowManualBlockerAction
} from './statusView'
import {
  renderPluginImportResult,
  renderPluginManagement,
  renderSettingsShell,
  type SettingsPanel
} from './settingsView'
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
import { pluginManagementSnapshotsEqual } from './pluginSnapshot'
import { formatLocalTime } from './viewUtils'
import { shouldReturnToDashboardForState } from './dashboardAutoReturn'
import { optimisticPluginRuntimeStatus, type ActiveView } from './appState'
import { normalizeListenerTools, portFromUrl } from './dashboardController'
import { createLanguageController } from './languageController'
import { collectPluginConfig, cssEscape, delay, pluginActionKey } from './pluginController'
import { notificationPlugins } from './settingsController'
import { createStateStreamClient, type StateStreamClient } from './sseClient'
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
let streamFallbackTimer: number | undefined
let stateStreamClient: StateStreamClient | undefined
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
let pluginActionBusyKey: string | null = null
let pluginConfigBusyId: string | null = null
let notificationTestBusyPluginId: string | null = null
let notificationTestResultPluginId: string | null = null
let pendingPluginTransition: PluginTransitionTarget | null = null
let pluginImportBusy = false
let pluginImportResultText = ''
let pluginActionResultText = ''
let pluginConfigResultText = ''
let activeView: ActiveView = 'dashboard'
let activeSettingsPanel: SettingsPanel = 'plugins'
let notificationRecords: NotificationRecord[] = []
let notificationRecordsLoaded = false
let localApiUrlText = ''
let localSseConnected = false
let approvingApprovalRequestId: string | null = null
let submittingInputRequestId: string | null = null
const CUSTOM_INPUT_VALUE = '__niuma_custom_answer__'

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
const statusSummaryEl = document.querySelector<HTMLElement>('#status-summary')
const updatedEl = document.querySelector<HTMLElement>('#updated')
const toolListenerListEl = document.querySelector<HTMLElement>('#tool-listener-list')
const codexListenerDescriptionEl = document.querySelector<HTMLElement>(
  '#codex-listener-description'
)
const requestDetailEl = document.querySelector<HTMLDListElement>('#request-detail')
const approvalActionsEl = document.querySelector<HTMLElement>('#approval-actions')
const refreshButton = document.querySelector<HTMLButtonElement>('#refresh')
const clearBlockerButton = document.querySelector<HTMLButtonElement>('#clear-blocker')

const languageController = createLanguageController({
  eventName: languageChangedEvent,
  selectElement: languageSelectEl,
  getLanguage: () => currentLanguage,
  setLanguage: (language) => {
    currentLanguage = language
  },
  renderLanguage: applyLanguage,
  reportError: (error) => {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  }
})

languageController.setupSelect()
applyLanguage()
languageController.setupRuntimeSync()
languageController.syncLanguageFromRuntime()

async function refreshDashboard() {
  renderStatePayload(await refreshMainState())
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
  if (!applyPluginSnapshot(data.list)) {
    return
  }
  renderPluginSettings()
  renderNotificationSettings()
}

function startPluginRuntimeRefresh() {
  if (!pluginRuntimeRefresh) {
    pluginRuntimeRefresh = createPluginRuntimeRefresh({
      intervalMs: pluginRuntimeRefreshIntervalMs,
      isActive: () => activeView === 'settings',
      refresh: refreshPluginRuntimeSnapshot
    })
  }
  // 插件运行态保存在内存中，主状态 SSE 不一定变化；用轻量轮询同步界面显示。
  pluginRuntimeRefresh.start()
}

function applyPluginSnapshot(snapshot: PluginManagementItem[]) {
  const nextPlugins = snapshot.map((plugin) =>
    mergePendingPluginTransition(plugin, pendingPluginTransition)
  )
  const changed = !pluginManagementSnapshotsEqual(plugins, nextPlugins)
  plugins = nextPlugins
  return changed
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
    clearBlockerButton!.hidden = !shouldShowManualBlockerAction(latestMainState)
    renderStatusSummary({
      element: statusSummaryEl,
      state: latestMainState,
      language: currentLanguage
    })
    renderRequestDetail({
      element: requestDetailEl,
      actionsElement: approvalActionsEl,
      state: latestMainState,
      language: currentLanguage,
      approving: latestMainState.detail?.interaction?.request_id === approvingApprovalRequestId,
      inputSubmitting: latestMainState.detail?.interaction?.request_id === submittingInputRequestId
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
      actionsElement: approvalActionsEl,
      state: null,
      language: currentLanguage,
      approving: false,
      inputSubmitting: false
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
}

function renderPluginSettings() {
  renderPluginManagement({
    element: document.querySelector<HTMLElement>('#plugin-management-list'),
    language: currentLanguage,
    plugins,
    busyPluginId: pluginBusyId,
    busyActionKey: pluginActionBusyKey,
    busyConfigPluginId: pluginConfigBusyId,
    importBusy: pluginImportBusy,
    resultText: pluginImportResultText,
    actionResultText: pluginActionResultText,
    configResultText: pluginConfigResultText,
    notificationTestBusyPluginId,
    notificationTestResultPluginId,
    notificationTestResultText: notificationResultText,
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

function renderNotificationPage() {
  renderNotificationPageView({
    formElement: notificationFormEl,
    settingsTitleElement: notificationSettingsTitleEl,
    language: currentLanguage,
    notificationPlugins: notificationPlugins(plugins),
    busyPluginId: pluginBusyId
  })
}

function renderNotificationSettings() {
  renderNotificationPage()
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
  clearBlockerButton!.textContent = clearBlockerNeedsConfirm
    ? t.clearBlockerConfirmAgain
    : blockerActionLabel(latestMainState, currentLanguage)
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
  renderDashboard()
  renderActiveView()
}

function renderStatePayload(payload: MainStatePayload) {
  const shouldReturnToDashboard = shouldReturnToDashboardForState({
    activeView,
    previousState: latestMainState,
    nextState: payload
  })
  latestMainState = payload
  if (shouldReturnToDashboard) {
    showDashboardView()
  }
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

dashboardViewEl?.addEventListener('click', async (event) => {
  const target = event.target instanceof HTMLElement ? event.target : null
  const button = target?.closest<HTMLButtonElement>('[data-approval-decision]')
  const requestId = button?.dataset.approvalRequestId
  const decision = button?.dataset.approvalDecision
  if (!button || !requestId || (decision !== 'allow' && decision !== 'deny')) {
    return
  }
  approvingApprovalRequestId = requestId
  renderDashboard()
  try {
    await submitApprovalDecision(requestId, decision)
    await refreshDashboard()
  } catch (error) {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  } finally {
    approvingApprovalRequestId = null
    renderDashboard()
  }
})

dashboardViewEl?.addEventListener('submit', async (event) => {
  const form =
    event.target instanceof HTMLFormElement
      ? event.target.closest<HTMLFormElement>('form[data-input-request-id]')
      : null
  if (!form) {
    return
  }
  event.preventDefault()
  const requestId = form.dataset.inputRequestId
  if (!requestId || submittingInputRequestId) {
    return
  }
  const interaction = latestMainState?.detail?.interaction
  const questions = interaction?.kind === 'input' ? interaction.schema?.questions : null
  const sessionId = latestMainState?.session?.id
  const wrapperSessionId = parseWrapperSessionId(requestId)
  if (!questions?.length || !sessionId || !wrapperSessionId) {
    updatedEl!.textContent = translations[currentLanguage].error
    return
  }
  if (!validateCustomInputAnswers(form, questions)) {
    return
  }
  submittingInputRequestId = requestId
  renderDashboard()
  try {
    await submitInputAnswer(
      sessionId,
      wrapperSessionId,
      requestId,
      collectInputAnswers(form, questions)
    )
    await refreshDashboard()
  } catch (error) {
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  } finally {
    submittingInputRequestId = null
    renderDashboard()
  }
})

function parseWrapperSessionId(requestId: string) {
  // Codex input request_id 内嵌 wrapper session id，提交答案时后端需要它定位 wrapper 会话。
  return requestId.match(/codex-input:(niuma_codex_[^:]+):/)?.[1] ?? null
}

function collectInputAnswers(
  form: HTMLFormElement,
  questions: EventInteractionQuestion[]
) {
  const formData = new FormData(form)
  const answers: Record<string, string[]> = {}
  for (const question of questions) {
    const selected = formData.get(question.id)
    const rawValues =
      selected === CUSTOM_INPUT_VALUE
        ? formData.getAll(customInputName(question.id))
        : formData.getAll(question.id)
    const values = rawValues
      .map(String)
      .map((value) => value.trim())
      .filter((value) => value !== '' && value !== CUSTOM_INPUT_VALUE)
    if (values.length > 0) {
      answers[question.id] = values
    }
  }
  return answers
}

function validateCustomInputAnswers(
  form: HTMLFormElement,
  questions: EventInteractionQuestion[]
) {
  const formData = new FormData(form)
  for (const question of questions) {
    const textarea = form.querySelector<HTMLTextAreaElement>(
      `textarea[data-custom-input-for="${cssEscape(question.id)}"]`
    )
    if (!textarea) {
      continue
    }
    textarea.required = formData.get(question.id) === CUSTOM_INPUT_VALUE
    if (textarea.required && textarea.value.trim() === '') {
      textarea.reportValidity()
      return false
    }
  }
  return true
}

function customInputName(questionId: string) {
  return `${questionId}__custom`
}

settingsViewEl?.addEventListener('click', async (event) => {
  const target = event.target instanceof HTMLElement ? event.target : null
  const t = translations[currentLanguage]
  const settingsPanel = target?.dataset.settingsPanel
  if (settingsPanel === 'plugins' || settingsPanel === 'notification-history') {
    if (settingsPanel === activeSettingsPanel) {
      return
    }
    activeSettingsPanel = settingsPanel
    renderSettings()
    if (activeSettingsPanel === 'notification-history' && !notificationRecordsLoaded) {
      await refreshNotificationRecords()
    }
    return
  }
  if (target?.id === 'settings-notification-history-refresh') {
    await refreshNotificationRecords()
    return
  }
  const notificationTestButton = target?.closest<HTMLButtonElement>(
    '[data-plugin-notification-test]'
  )
  const notificationTestPluginId = notificationTestButton?.dataset.pluginNotificationTest
  if (notificationTestPluginId) {
    if (pluginBusyId || notificationTestBusyPluginId) {
      return
    }
    const t = translations[currentLanguage]
    notificationTestBusyPluginId = notificationTestPluginId
    notificationTestResultPluginId = notificationTestPluginId
    notificationResultText = t.sending
    renderPluginSettings()
    try {
      await refreshPlugins()
      const plugin = plugins.find((item) => item.id === notificationTestPluginId)
      if (
        !plugin ||
        plugin.kind !== 'notification' ||
        !plugin.capabilities.includes('notification_test')
      ) {
        notificationResultText = t.noChannelsEnabled
        return
      }
      // 测试通知属于单个插件的能力，入口放在插件管理卡片中。
      await sendTestNotification(notificationTestPluginId)
      notificationResultText = formatNotificationTestResult(currentLanguage, [
        notificationTestPluginId
      ], [])
    } catch (error) {
      notificationResultText = formatNotificationTestResult(currentLanguage, [], [
        {
          pluginId: notificationTestPluginId,
          message: error instanceof Error ? error.message : String(error)
        }
      ])
    } finally {
      notificationTestBusyPluginId = null
      renderPluginSettings()
      renderNotificationSettings()
    }
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
  const pluginActionButton = target?.closest<HTMLElement>('[data-plugin-action-id]')
  const pluginActionPluginId = pluginActionButton?.dataset.pluginActionPlugin
  const pluginActionId = pluginActionButton?.dataset.pluginActionId
  if (pluginActionPluginId && pluginActionId) {
    const actionKey = pluginActionKey(pluginActionPluginId, pluginActionId)
    pluginActionBusyKey = actionKey
    pluginActionResultText = ''
    renderPluginSettings()
    try {
      const result = await runPluginAction(pluginActionPluginId, pluginActionId)
      applyPluginSnapshot(result.plugins)
      pluginActionResultText = result.message
    } catch (error) {
      pluginActionResultText = error instanceof Error ? error.message : String(error)
    } finally {
      pluginActionBusyKey = null
      renderPluginSettings()
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
        optimisticStatus: optimisticPluginRuntimeStatus(nextEnabled)
      }
    : null
  plugins = plugins.map((plugin) =>
    plugin.id === pluginId
      ? {
          ...plugin,
          enabled: nextEnabled,
          runtime_status: tracksRuntimeTransition
            ? optimisticPluginRuntimeStatus(nextEnabled)
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
    optimisticStatus: optimisticPluginRuntimeStatus(nextEnabled)
  }
  plugins = plugins.map((plugin) =>
    plugin.id === pluginId
      ? {
          ...plugin,
          enabled: nextEnabled,
          runtime_status: optimisticPluginRuntimeStatus(nextEnabled)
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
    updatedEl!.textContent = error instanceof Error ? error.message : String(error)
  } finally {
    pendingPluginTransition = null
    pluginBusyId = null
    renderNotificationSettings()
    renderPluginSettings()
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
    clearBlockerButton.textContent = blockerActionLabel(latestMainState, currentLanguage)
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

async function startStream() {
  try {
    const apiUrl = await getLocalApiUrl()
    localApiUrlText = apiUrl
    renderLocalSseStatus()
    stateStreamClient = createStateStreamClient<MainStatePayload>({
      url: `${apiUrl}${stateStreamPath}`,
      fallbackIntervalMs: 2_000,
      onConnected: (connected) => {
        localSseConnected = connected
        renderLocalSseStatus()
      },
      onState: renderStatePayload,
      onFallback: refreshDashboard,
      onFallbackError: (error) => {
        updatedEl!.textContent = error instanceof Error ? error.message : String(error)
      }
    })
    stateStreamClient.start()
    stopStreamFallbackPolling()
  } catch {
    localSseConnected = false
    renderLocalSseStatus()
    startStreamFallbackPolling()
  }
}

function startStreamFallbackPolling() {
  if (streamFallbackTimer !== undefined) {
    return
  }
  streamFallbackTimer = window.setInterval(() => {
    refreshDashboard().catch((error) => {
      updatedEl!.textContent = error instanceof Error ? error.message : String(error)
    })
  }, 2_000)
}

function stopStreamFallbackPolling() {
  if (streamFallbackTimer === undefined) {
    return
  }
  window.clearInterval(streamFallbackTimer)
  streamFallbackTimer = undefined
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
  stateStreamClient?.stop()
  stopStreamFallbackPolling()
  pluginRuntimeRefresh?.stop()
})
