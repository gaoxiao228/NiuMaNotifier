import type {
  NotificationChannel,
  NotificationChannelConfig,
  NotificationRecord,
} from './api'
import {
  translateEventType,
  translateNotificationReason,
  translateNotificationStatus,
  translations,
  type LanguageCode
} from './i18n'
import { escapeHtml, formatLocalTime } from './viewUtils'

const barkIconUrl = '/assets/bark-icon.png'
const ntfyIconUrl = '/assets/ntfy-logo.svg'

export type NotificationPageRenderOptions = {
  formElement: HTMLElement | null
  settingsTitleElement: HTMLElement | null
  language: LanguageCode
  channels: NotificationChannelConfig[]
  resultText: string
  busyChannel: NotificationChannel | null
}

export type NotificationHistoryRenderOptions = {
  historyElement: HTMLOListElement | null
  language: LanguageCode
  records: NotificationRecord[]
  recordsLoaded: boolean
}

export type NotificationTestFailure = {
  channel: NotificationChannel
  message: string
}

export function renderNotificationPage(options: NotificationPageRenderOptions) {
  const t = translations[options.language]
  if (!options.formElement) {
    return
  }
  const bark = channelConfig(options.channels, 'bark')
  const ntfy = channelConfig(options.channels, 'ntfy')
  renderNotificationTitles(options)
  options.formElement.innerHTML = `
    ${renderChannelForm(options, 'bark', t.barkSettings, bark, ['device_key'])}
    ${renderChannelForm(options, 'ntfy', t.ntfySettings, ntfy, ['topic'])}
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
  sentChannels: NotificationChannel[],
  failedChannels: NotificationTestFailure[]
) {
  const t = translations[language]
  const parts: string[] = []
  if (sentChannels.length > 0) {
    parts.push(`${t.sent}: ${sentChannels.join(' / ')}`)
  }
  if (failedChannels.length > 0) {
    parts.push(
      `${t.error}: ${failedChannels
        .map((item) => `${item.channel}: ${item.message}`)
        .join(' / ')}`
    )
  }
  return parts.join('; ')
}

export function collectNotificationChannels(
  formElement: HTMLElement | null
): NotificationChannelConfig[] {
  return (['bark', 'ntfy'] as NotificationChannel[]).map((channel) => {
    const root = formElement?.querySelector<HTMLElement>(
      `.notification-channel[data-channel="${channel}"]`
    )
    const payload: Record<string, unknown> = { secret_ref: null }
    root?.querySelectorAll<HTMLInputElement>('input[data-field]').forEach((input) => {
      const field = input.dataset.field
      if (field && field !== 'enabled') {
        payload[field] = input.value
      }
    })
    return {
      channel,
      enabled: root?.querySelector<HTMLInputElement>('input[data-field="enabled"]')?.checked ?? false,
      payload
    }
  })
}

export function renderNotificationHistoryOnly(options: NotificationHistoryRenderOptions) {
  renderNotificationHistory(options)
}

function renderNotificationTitles(options: NotificationPageRenderOptions) {
  const t = translations[options.language]
  if (options.settingsTitleElement) {
    options.settingsTitleElement.textContent = t.notificationSettings
  }
}

function channelConfig(
  channels: NotificationChannelConfig[],
  channel: NotificationChannel
): NotificationChannelConfig {
  return (
    channels.find((item) => item.channel === channel) ?? {
      channel,
      enabled: false,
      payload: {}
    }
  )
}

function renderChannelForm(
  options: NotificationPageRenderOptions,
  channel: NotificationChannel,
  title: string,
  config: NotificationChannelConfig,
  fields: string[]
) {
  const t = translations[options.language]
  const payload = config.payload as Record<string, string>
  const fieldLabels: Record<string, string> = {
    server: t.server,
    device_key: t.deviceKey,
    group: t.group,
    topic: t.topic,
    token: t.token
  }
  const iconUrl = channel === 'bark' ? barkIconUrl : ntfyIconUrl
  return `
    <section class="notification-channel" data-channel="${channel}">
      <div class="notification-channel-heading">
        <span class="notification-channel-title">
          <img src="${escapeHtml(iconUrl)}" alt="" aria-hidden="true">
          <h3 class="notification-compact-title">${escapeHtml(title)}</h3>
        </span>
        <label class="notification-enable">
          <input type="checkbox" data-field="enabled" ${config.enabled ? 'checked' : ''}>
          <span>${escapeHtml(t.enabled)}</span>
        </label>
      </div>
      ${fields
        .map(
          (field) => `
            <label class="notification-field-row">
              <span>${escapeHtml(fieldLabels[field])}</span>
              <input type="text" data-field="${field}" value="${escapeHtml(
                String(payload[field] ?? '')
              )}">
            </label>
          `
        )
        .join('')}
    </section>
  `
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
