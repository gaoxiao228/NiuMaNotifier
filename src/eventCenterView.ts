import type { NiumaEvent } from './api'
import {
  translateEventType,
  translateTool,
  translations,
  type LanguageCode
} from './i18n'
import { escapeHtml, formatLocalTime } from './viewUtils'

export type EventCenterRenderOptions = {
  element: HTMLElement | null
  language: LanguageCode
  events: NiumaEvent[]
  expandedEventIds: Set<string>
  connected: boolean
  connecting: boolean
  errorText: string
}

export function renderEventCenter(options: EventCenterRenderOptions) {
  if (!options.element) {
    return
  }
  const t = translations[options.language]
  const statusText = options.connected
    ? t.eventCenterConnected
    : options.connecting
      ? t.eventCenterConnecting
      : t.eventCenterDisconnected
  const statusClass = options.connected ? 'connected' : options.connecting ? 'connecting' : 'disconnected'
  options.element.innerHTML = `
    <div class="event-center-status-row">
      <span class="event-center-status ${statusClass}">${escapeHtml(statusText)}</span>
      ${options.errorText ? `<span class="event-center-error">${escapeHtml(options.errorText)}</span>` : ''}
    </div>
    <ol class="event-center-list">
      ${renderEventCenterItems(options)}
    </ol>
  `
}

function renderEventCenterItems(options: EventCenterRenderOptions) {
  const t = translations[options.language]
  if (options.events.length === 0) {
    return `<li class="empty">${escapeHtml(t.eventCenterWaiting)}</li>`
  }
  return options.events.map((event) => renderEventCenterItem(event, options)).join('')
}

function renderEventCenterItem(event: NiumaEvent, options: EventCenterRenderOptions) {
  const expanded = options.expandedEventIds.has(event.id)
  const detail = expanded
    ? `<div class="event-center-detail"><pre class="event-center-json">${escapeHtml(JSON.stringify(event, null, 2))}</pre></div>`
    : ''
  // 每条事件只把摘要放在折叠行，完整原始字段统一交给 JSON 详情区展示。
  return `
    <li class="event-center-item ${expanded ? 'expanded' : ''}">
      <button class="event-center-row" type="button" data-event-center-toggle="${escapeHtml(event.id)}" aria-expanded="${expanded}">
        <strong>${escapeHtml(translateEventType(options.language, event.event_type))}</strong>
        <span>${escapeHtml(translateTool(options.language, event.tool))}</span>
        <span>${escapeHtml(event.project_name || translations[options.language].none)}</span>
        <span class="event-center-summary">${escapeHtml(event.summary || translations[options.language].none)}</span>
        <time>${escapeHtml(formatLocalTime(event.created_at, options.language))}</time>
      </button>
      ${detail}
    </li>
  `
}
