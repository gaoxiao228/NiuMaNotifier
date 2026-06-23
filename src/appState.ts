import type {
  ListenerToolConfig,
  MainStatePayload,
  NotificationRecord,
  PluginManagementItem,
  PluginRuntimeStatus
} from './api'
import type { PluginTransitionTarget } from './pluginTransition'
import type { SettingsPanel } from './settingsView'

export type ActiveView = 'dashboard' | 'settings'

export interface AppRuntimeState {
  latestMainState: MainStatePayload | null
  listenerTools: ListenerToolConfig[]
  listenerConfigLoaded: boolean
  listenerBusyToolId: string | null
  plugins: PluginManagementItem[]
  pluginConfigs: Record<string, Record<string, unknown>>
  pluginBusyId: string | null
  pluginActionBusyKey: string | null
  pluginConfigBusyId: string | null
  notificationTestBusyPluginId: string | null
  notificationTestResultPluginId: string | null
  pendingPluginTransition: PluginTransitionTarget | null
  pluginImportBusy: boolean
  pluginImportResultText: string
  pluginActionResultText: string
  pluginConfigResultText: string
  activeView: ActiveView
  activeSettingsPanel: SettingsPanel
  notificationRecords: NotificationRecord[]
  notificationRecordsLoaded: boolean
  localApiUrlText: string
  localSseConnected: boolean
  approvingApprovalRequestId: string | null
  clearBlockerNeedsConfirm: boolean
  notificationResultText: string
}

export function createAppRuntimeState(): AppRuntimeState {
  return {
    latestMainState: null,
    listenerTools: [],
    listenerConfigLoaded: false,
    listenerBusyToolId: null,
    plugins: [],
    pluginConfigs: {},
    pluginBusyId: null,
    pluginActionBusyKey: null,
    pluginConfigBusyId: null,
    notificationTestBusyPluginId: null,
    notificationTestResultPluginId: null,
    pendingPluginTransition: null,
    pluginImportBusy: false,
    pluginImportResultText: '',
    pluginActionResultText: '',
    pluginConfigResultText: '',
    activeView: 'dashboard',
    activeSettingsPanel: 'plugins',
    notificationRecords: [],
    notificationRecordsLoaded: false,
    localApiUrlText: '',
    localSseConnected: false,
    approvingApprovalRequestId: null,
    clearBlockerNeedsConfirm: false,
    notificationResultText: ''
  }
}

export function optimisticPluginRuntimeStatus(enabled: boolean): PluginRuntimeStatus {
  return enabled ? 'starting' : 'stopping'
}
