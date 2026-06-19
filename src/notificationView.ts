import type {
  NotificationRecord,
  PluginManagementItem,
} from './api'
import {
  translateEventType,
  translateNotificationReason,
  translateNotificationStatus,
  translations,
  type LanguageCode
} from './i18n'
import { escapeHtml, formatLocalTime } from './viewUtils'

export type NotificationPageRenderOptions = {
  formElement: HTMLElement | null
  settingsTitleElement: HTMLElement | null
  language: LanguageCode
  notificationPlugins: PluginManagementItem[]
  resultText: string
  busyPluginId: string | null
}

export type NotificationHistoryRenderOptions = {
  historyElement: HTMLOListElement | null
  language: LanguageCode
  records: NotificationRecord[]
  recordsLoaded: boolean
}

export type NotificationTestFailure = {
  pluginId: string
  message: string
}

export function renderNotificationPage(options: NotificationPageRenderOptions) {
  const t = translations[options.language]
  if (!options.formElement) {
    return
  }
  renderNotificationTitles(options)
  const pluginList =
    options.notificationPlugins.length > 0
      ? options.notificationPlugins.map((plugin) => renderNotificationPluginItem(plugin, options)).join('')
      : `<p class="empty">${escapeHtml(t.noPlugins)}</p>`
  options.formElement.innerHTML = `
    <div class="notification-plugin-list">
      ${pluginList}
    </div>
    <p class="notification-result">${escapeHtml(t.lastResult)}: ${escapeHtml(
      options.resultText || t.none
    )}</p>
  `
}

export function renderNotificationResult(
  formElement: HTMLElement | null,
  language: LanguageCode,
  resultText: string
) {
  const resultElement = formElement?.querySelector<HTMLElement>('.notification-result')
  if (!resultElement) {
    return
  }
  const t = translations[language]
  resultElement.textContent = `${t.lastResult}: ${resultText || t.none}`
}

export function formatNotificationTestResult(
  language: LanguageCode,
  sentPluginIds: string[],
  failedPlugins: NotificationTestFailure[]
) {
  const t = translations[language]
  const parts: string[] = []
  if (sentPluginIds.length > 0) {
    parts.push(`${t.sent}: ${sentPluginIds.join(' / ')}`)
  }
  if (failedPlugins.length > 0) {
    parts.push(
      `${t.error}: ${failedPlugins
        .map((item) => `${item.pluginId}: ${item.message}`)
        .join(' / ')}`
    )
  }
  return parts.join('; ')
}

export function renderNotificationHistoryOnly(options: NotificationHistoryRenderOptions) {
  renderNotificationHistory(options)
}

function renderNotificationTitles(options: NotificationPageRenderOptions) {
  const t = translations[options.language]
  if (options.settingsTitleElement) {
    options.settingsTitleElement.textContent = t.notificationPlugins
  }
}

function renderNotificationPluginItem(
  plugin: PluginManagementItem,
  options: NotificationPageRenderOptions
) {
  const t = translations[options.language]
  const busy = options.busyPluginId === plugin.id || isPluginTransitioning(plugin)
  const enabledText = plugin.enabled ? t.enabled : t.disabled
  // 主界面只展示通知插件运行摘要，具体 key/value 配置统一放在插件管理。
  return `
    <article class="notification-plugin-item" data-notification-plugin-id="${escapeHtml(plugin.id)}">
      <div class="notification-plugin-copy">
        <strong>${escapeHtml(plugin.display_name)}</strong>
        <span>${escapeHtml(plugin.id)}</span>
      </div>
      <div class="notification-plugin-state">
        <span class="notification-plugin-runtime ${escapeHtml(plugin.runtime_status)}">${escapeHtml(
          translateRuntimeStatus(options.language, plugin.runtime_status)
        )}</span>
        <label class="notification-enable">
          <span>${escapeHtml(enabledText)}</span>
          <input type="checkbox" data-notification-plugin-toggle="${escapeHtml(plugin.id)}" ${
            plugin.enabled ? 'checked' : ''
          } ${busy ? 'disabled' : ''}>
        </label>
      </div>
    </article>
  `
}

function translateRuntimeStatus(language: LanguageCode, status: string) {
  const t = translations[language]
  if (status === 'starting') {
    return t.pluginStarting
  }
  if (status === 'running') {
    return t.pluginRunning
  }
  if (status === 'stopping') {
    return t.pluginStopping
  }
  if (status === 'failed') {
    return t.pluginFailed
  }
  return t.pluginStopped
}

function isPluginTransitioning(plugin: PluginManagementItem) {
  return plugin.runtime_status === 'starting' || plugin.runtime_status === 'stopping'
}

function renderNotificationHistory(options: NotificationHistoryRenderOptions) {
  const t = translations[options.language]
  const element = options.historyElement
  if (!element) {
    return
  }
  if (!options.recordsLoaded) {
    element.innerHTML = `<li class="empty">${escapeHtml(t.loading)}</li>`
    return
  }
  if (options.records.length === 0) {
    element.innerHTML = `<li class="empty">${escapeHtml(t.noNotificationRecords)}</li>`
    return
  }
  element.innerHTML = options.records
    .map((record) => {
      const error = record.error_message ? `<p>${escapeHtml(record.error_message)}</p>` : ''
      const titleRows = record.title
        ? `
            <dt>${escapeHtml(t.notificationTitle)}</dt>
            <dd>${escapeHtml(record.title)}</dd>
          `
        : ''
      const body = record.body
        ? `<div class="notification-record-body">${escapeHtml(record.body)}</div>`
        : ''
      return `
        <li>
          <div class="notification-record-row">
            <strong>${escapeHtml(translateEventType(options.language, record.event_type))}</strong>
            <span class="notification-record-status ${escapeHtml(record.status)}">${escapeHtml(
              translateNotificationStatus(options.language, record.status)
            )}</span>
          </div>
          <dl>
            <dt>${escapeHtml(t.notificationChannel)}</dt>
            <dd>${escapeHtml(record.channel)}</dd>
            ${titleRows}
            <dt>${escapeHtml(t.notificationReason)}</dt>
            <dd>${escapeHtml(translateNotificationReason(options.language, record.reason))}</dd>
            <dt>${escapeHtml(t.notificationCreatedAt)}</dt>
            <dd>${escapeHtml(formatLocalTime(record.created_at, options.language))}</dd>
            <dt>${escapeHtml(t.notificationSentAt)}</dt>
            <dd>${escapeHtml(record.sent_at ? formatLocalTime(record.sent_at, options.language) : t.none)}</dd>
          </dl>
          ${body ? `<h4>${escapeHtml(t.notificationContent)}</h4>${body}` : ''}
          ${error ? `<h4>${escapeHtml(t.notificationError)}</h4>${error}` : ''}
        </li>
      `
    })
    .join('')
}
