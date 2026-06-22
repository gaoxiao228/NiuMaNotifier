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
import { renderPluginIcon } from './pluginIcon'
import { escapeHtml, formatLocalTime } from './viewUtils'

export type NotificationPageRenderOptions = {
  formElement: HTMLElement | null
  settingsTitleElement: HTMLElement | null
  language: LanguageCode
  notificationPlugins: PluginManagementItem[]
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
  `
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
      ${renderPluginIcon(plugin)}
      <div class="notification-plugin-copy">
        <strong>${escapeHtml(plugin.display_name)} <span class="notification-plugin-runtime ${escapeHtml(
          plugin.runtime_status
        )}">${escapeHtml(translateRuntimeStatus(options.language, plugin.runtime_status))}</span></strong>
        <span>${escapeHtml(plugin.id)}</span>
      </div>
      <div class="notification-plugin-state">
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
      const title = record.title || translateEventType(options.language, record.event_type)
      const body = record.body || record.error_message || ''
      const metaLabelSuffix = options.language === 'en' || options.language === 'de' ? ':' : '：'
      const detailClass = record.error_message
        ? 'notification-record-detail error'
        : 'notification-record-detail'
      return `
        <li class="notification-record-card">
          <div class="notification-record-header">
            <strong>${escapeHtml(title)}</strong>
            <span class="notification-record-status ${escapeHtml(record.status)}">${escapeHtml(
              translateNotificationStatus(options.language, record.status)
            )}</span>
            <span class="notification-record-channel">${escapeHtml(record.channel)}</span>
            <span class="notification-record-title">${escapeHtml(
              translateEventType(options.language, record.event_type)
            )}</span>
          </div>
          <dl class="notification-record-meta">
            <dt>${escapeHtml(`${t.notificationReason}${metaLabelSuffix}`)}</dt>
            <dd>${escapeHtml(translateNotificationReason(options.language, record.reason))}</dd>
            <dt>${escapeHtml(`${t.notificationCreatedAt}${metaLabelSuffix}`)}</dt>
            <dd>${escapeHtml(formatLocalTime(record.created_at, options.language))}</dd>
            <dt>${escapeHtml(`${t.notificationSentAt}${metaLabelSuffix}`)}</dt>
            <dd>${escapeHtml(record.sent_at ? formatLocalTime(record.sent_at, options.language) : t.none)}</dd>
          </dl>
          ${
            body
              ? `<div class="${detailClass}">${escapeHtml(body)}</div>`
              : `<div class="notification-record-detail empty">${escapeHtml(t.none)}</div>`
          }
        </li>
      `
    })
    .join('')
}
