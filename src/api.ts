import { invoke } from '@tauri-apps/api/core'

export type ApiResponse<T> = {
  code: number
  message: string
  data: T
}

export type NiumaEvent = {
  id: string
  tool: string
  session_id: string
  project_name: string
  project_path: string
  event_type: string
  severity: string
  summary: string
  content?: string | null
  error_message?: string | null
  created_at: string
}

export type MainStatePayload = {
  version: number
  status: string
  updated_at: string | null
  session: MainStateSession | null
  detail: MainStateDetail | null
}

export type MainStateSession = {
  id: string
  tool: string
  project_name: string
  project_path: string
}

export type MainStateDetail = {
  event_id: string
  event_type: string
  severity: string
  summary: string
  content: string | null
  error_message: string | null
  payload_ref: string | null
  completion_reason: string | null
  failure_reason: string | null
}

export type NiumaSession = {
  id: string
  tool: string
  project_path: string
  project_name: string
  status: string
  last_event_id: string | null
  last_activity_at: string
}

export type SessionsPayload = {
  list: NiumaSession[]
}

export type RecentEvents = {
  list: NiumaEvent[]
  warning?: string
}

export type ListenerConfigPayload = {
  codex_listening_enabled: boolean
  tool_listening_enabled?: Record<string, boolean>
  tools?: ListenerToolConfig[]
}

export type ListenerToolConfig = {
  id: string
  plugin_id: string
  display_name: string
  enabled: boolean
  source: string
  icon_url?: string | null
}

export type PluginRuntimeStatus = 'starting' | 'stopped' | 'stopping' | 'running' | 'failed'

export type PluginConfigFieldType = 'string' | 'secret' | 'url' | 'number' | 'boolean' | 'select'

export type PluginConfigField = {
  key: string
  type: PluginConfigFieldType
  label: string
  required?: boolean
  default?: unknown
  options?: string[]
}

export type PluginManagementItem = {
  id: string
  kind?: 'tool' | 'notification' | 'status_indicator'
  tool_id: string | null
  display_name: string
  version: string
  source: string
  capabilities: string[]
  enabled: boolean
  runtime_status: PluginRuntimeStatus
  last_error: string | null
  icon_url: string | null
  config_schema: PluginConfigField[]
  install_path: string | null
}

export type PluginsPayload = {
  list: PluginManagementItem[]
}

export type PluginImportResult = {
  imported: boolean
  cancelled?: boolean
  plugin?: PluginManagementItem
  plugins: PluginManagementItem[]
}

export type PluginRemoveResult = {
  removed: boolean
  plugin_id: string
  plugins: PluginManagementItem[]
}

export type PluginEnabledResult = {
  saved: boolean
  plugin_id: string
  enabled: boolean
  plugins: PluginManagementItem[]
}

export type PluginConfigPayload = {
  plugin_id: string
  config: Record<string, unknown>
  config_schema: PluginConfigField[]
}

export type PluginConfigSaveResult = PluginConfigPayload & {
  saved: boolean
}

export type TestNotificationResult = {
  sent: boolean
  plugin_id: string
  test_id: string
  record_id: string
}

export type NotificationRecordStatus = 'pending' | 'sent' | 'failed' | 'skipped'

export type NotificationRecord = {
  id: string
  event_id: string
  event_type: string
  channel: string
  plugin_id?: string | null
  status: NotificationRecordStatus
  title: string | null
  body: string | null
  reason: string | null
  error_message: string | null
  created_at: string
  sent_at: string | null
}

export type NotificationRecordsPayload = {
  list: NotificationRecord[]
}

type LocalApiInfo = {
  url: string
}

type ActiveLanguageInfo = {
  language: string
  preference: string
}

let localApiUrl: string | undefined

async function requestLocalApi<T>(path: string, init?: RequestInit) {
  const apiUrl = await getLocalApiUrl()
  const response = await fetch(`${apiUrl}${path}`, init)
  const body = (await response.json()) as ApiResponse<T>
  if (body.code !== 0) {
    throw new Error(body.message)
  }
  return body.data
}

export async function refreshMainState() {
  try {
    return (await requestLocalApi<{ state: MainStatePayload }>('/api/v1/main-state')).state
  } catch {
    const response = await invoke<ApiResponse<{ state: MainStatePayload }>>('get_main_state')
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data.state
  }
}

export async function refreshSupplementaryData() {
  try {
    // 主状态由 state SSE 驱动；这里仅刷新列表类辅助数据。
    const [sessions, events] = await Promise.all([
      requestLocalApi<SessionsPayload>('/api/v1/sessions'),
      requestLocalApi<RecentEvents>('/api/v1/events?limit=10')
    ])
    return {
      sessions: sessions.list,
      events: events.list
    }
  } catch {
    const [sessionsResponse, eventsResponse] = await Promise.all([
      invoke<ApiResponse<SessionsPayload>>('get_sessions'),
      invoke<ApiResponse<RecentEvents>>('get_recent_events')
    ])
    if (sessionsResponse.code !== 0) {
      throw new Error(sessionsResponse.message)
    }
    if (eventsResponse.code !== 0) {
      throw new Error(eventsResponse.message)
    }
    return {
      sessions: sessionsResponse.data.list,
      events: eventsResponse.data.list
    }
  }
}

export async function dismissActiveBlocker() {
  try {
    const apiUrl = await getLocalApiUrl()
    const response = await fetch(`${apiUrl}/api/v1/blocker/dismiss`, { method: 'POST' })
    return (await response.json()) as ApiResponse<{ dismissed: boolean; dismissed_count: number }>
  } catch {
    // Local API 可能在开发阶段被旧进程占用或未启动，回退到 Tauri command 写同一份进程内应用状态。
    return invoke<ApiResponse<{ dismissed: boolean; dismissed_count: number }>>('dismiss_active_blocker')
  }
}

export async function getListenerConfig() {
  try {
    const response = await invoke<ApiResponse<ListenerConfigPayload>>('get_listener_config')
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  } catch {
    return await requestLocalApi<ListenerConfigPayload>('/api/v1/listener-config')
  }
}

export async function saveListenerConfig(config: ListenerConfigPayload) {
  try {
    const response = await invoke<
      ApiResponse<ListenerConfigPayload & { saved: boolean }>
    >('save_listener_config', {
      codexListeningEnabled: config.codex_listening_enabled,
      toolListeningEnabled: config.tool_listening_enabled
    })
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  } catch {
    return await requestLocalApi<ListenerConfigPayload & { saved: boolean }>(
      '/api/v1/listener-config/save',
      {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(config)
      }
    )
  }
}

export async function getPlugins() {
  try {
    const response = await invoke<ApiResponse<PluginsPayload>>('get_plugins')
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  } catch {
    return await requestLocalApi<PluginsPayload>('/api/v1/plugins')
  }
}

export async function importPluginDir(sourceDir: string) {
  return await requestLocalApi<PluginImportResult>('/api/v1/plugins/import', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ source_dir: sourceDir })
  })
}

export async function selectAndImportPluginDir() {
  const response = await invoke<ApiResponse<PluginImportResult>>('select_and_import_plugin_dir')
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data
}

export async function removePlugin(pluginId: string) {
  try {
    return await requestLocalApi<PluginRemoveResult>('/api/v1/plugins/remove', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ plugin_id: pluginId })
    })
  } catch {
    const response = await invoke<ApiResponse<PluginRemoveResult>>('remove_plugin', {
      pluginId
    })
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  }
}

export async function setPluginEnabled(pluginId: string, enabled: boolean) {
  try {
    return await requestLocalApi<PluginEnabledResult>('/api/v1/plugins/enabled', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ plugin_id: pluginId, enabled })
    })
  } catch {
    const response = await invoke<ApiResponse<PluginEnabledResult>>('set_plugin_enabled', {
      pluginId,
      enabled
    })
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  }
}

export async function getPluginConfig(pluginId: string) {
  try {
    return await requestLocalApi<PluginConfigPayload>(
      `/api/v1/plugins/config?plugin_id=${encodeURIComponent(pluginId)}`
    )
  } catch {
    const response = await invoke<ApiResponse<PluginConfigPayload>>('get_plugin_config', {
      pluginId
    })
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  }
}

export async function savePluginConfig(pluginId: string, config: Record<string, unknown>) {
  try {
    return await requestLocalApi<PluginConfigSaveResult>('/api/v1/plugins/config', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ plugin_id: pluginId, config })
    })
  } catch {
    const response = await invoke<ApiResponse<PluginConfigSaveResult>>('save_plugin_config', {
      pluginId,
      config
    })
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  }
}

export async function getNotificationRecords() {
  try {
    return await requestLocalApi<NotificationRecordsPayload>('/api/v1/notification-records')
  } catch {
    const response =
      await invoke<ApiResponse<NotificationRecordsPayload>>('get_notification_records')
    if (response.code !== 0) {
      throw new Error(response.message)
    }
    return response.data
  }
}

export async function sendTestNotification(pluginId: string) {
  const response = await invoke<ApiResponse<TestNotificationResult>>('send_test_notification', {
    pluginId
  })
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data
}

export async function getLocalApiUrl() {
  if (localApiUrl) {
    return localApiUrl
  }
  const response = await invoke<ApiResponse<LocalApiInfo>>('get_local_api_url')
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  localApiUrl = response.data.url
  return localApiUrl
}

export async function getActiveLanguage() {
  const response = await invoke<ApiResponse<ActiveLanguageInfo>>('get_active_language')
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data.language
}

export async function saveLanguagePreference(language: string) {
  const response = await invoke<ApiResponse<ActiveLanguageInfo & { saved: boolean }>>(
    'save_language_preference',
    { language }
  )
  if (response.code !== 0) {
    throw new Error(response.message)
  }
  return response.data.language
}
