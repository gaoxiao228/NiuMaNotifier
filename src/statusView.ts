import type { ListenerToolConfig, MainStatePayload, NiumaEvent, NiumaSession } from './api'
import {
  translateEventType,
  translateStatus,
  translateTool,
  translations,
  type LanguageCode
} from './i18n'
import { escapeHtml, formatLocalTime } from './viewUtils'

export type ListenerToggleRenderOptions = {
  toggle: HTMLInputElement | null
  label: HTMLElement | null
  state: HTMLElement | null
  language: LanguageCode
  busy: boolean
  enabled: boolean
  loaded: boolean
}

export type ListenerToolsRenderOptions = {
  element: HTMLElement | null
  tools: ListenerToolConfig[]
  language: LanguageCode
  busyToolId: string | null
  loaded: boolean
}

export type RequestDetailRenderOptions = {
  element: HTMLDListElement | null
  state: MainStatePayload | null
  language: LanguageCode
}

export type StatusSummaryRenderOptions = {
  element: HTMLElement | null
  state: MainStatePayload | null
  language: LanguageCode
}

export type SessionRenderOptions = {
  listElement: HTMLElement | null
  overviewElement: HTMLDListElement | null
  sessions: NiumaSession[]
  events: NiumaEvent[]
  selectedSessionId: string | null
  primarySessionId: string | null | undefined
  language: LanguageCode
}

export function isBlockingStatus(status: string) {
  return status === 'waiting_approval' || status === 'waiting_input' || status === 'error'
}

export function statusTone(status: string) {
  if (status === 'running') {
    return 'warning'
  }
  if (status === 'completed' || status === 'idle') {
    return 'info'
  }
  return 'danger'
}

export function renderStatusSummary(options: StatusSummaryRenderOptions) {
  if (!options.element) {
    return
  }
  const t = translations[options.language]
  const status = options.state?.status ?? 'loading'
  const translated = options.state ? translateStatus(options.language, status) : t.loading
  const tone = options.state ? statusTone(status) : 'info'

  // 主状态卡片只展示状态本身，具体请求内容由下方详情区域承载，避免显示 raw status。
  options.element.className = `status-summary ${tone}`
  options.element.innerHTML = `
    <div class="status-line">
      <span class="status-icon" aria-hidden="true"></span>
      <strong>${escapeHtml(translated)}</strong>
      ${isBlockingStatus(status) ? `<span class="status-chip ${tone}">${escapeHtml(t.needsHandling)}</span>` : ''}
    </div>
  `
}

export function renderListenerToggle(options: ListenerToggleRenderOptions) {
  if (!options.toggle || !options.label || !options.state) {
    return
  }
  const t = translations[options.language]
  options.label.textContent = t.codexListening
  // 标题颜色跟随监听状态，避免关闭监听时仍显示绿色在线态。
  options.label.className = options.enabled ? 'listener-toggle-title enabled' : 'listener-toggle-title'
  options.state.textContent = options.busy
    ? t.listenerSaving
    : options.enabled
      ? t.codexListeningOn
      : t.codexListeningOff
  options.toggle.checked = options.enabled
  options.toggle.disabled = options.busy || !options.loaded
  options.toggle.setAttribute('aria-label', t.codexListening)
}

export function renderListenerTools(options: ListenerToolsRenderOptions) {
  if (!options.element) {
    return
  }
  const t = translations[options.language]
  const tools = options.tools.length > 0 ? options.tools : fallbackCodexTool(false)
  options.element.innerHTML = tools
    .map((tool) => {
      const busy = options.busyToolId === tool.id
      const titleClass = tool.enabled ? 'listener-toggle-title enabled' : 'listener-toggle-title'
      const stateText = busy ? t.listenerSaving : tool.enabled ? t.codexListeningOn : t.codexListeningOff
      return `
        <label class="listener-toggle" data-tool-id="${escapeHtml(tool.id)}">
          <span class="listener-toggle-copy">
            <strong class="${titleClass}">${escapeHtml(tool.display_name || translateTool(options.language, tool.id))}</strong>
            <span>${escapeHtml(stateText)}</span>
          </span>
          <input type="checkbox" data-tool-toggle="${escapeHtml(tool.id)}" ${tool.enabled ? 'checked' : ''} ${busy || !options.loaded ? 'disabled' : ''} aria-label="${escapeHtml(tool.display_name)}">
        </label>
      `
    })
    .join('')
}

function fallbackCodexTool(enabled: boolean): ListenerToolConfig[] {
  return [
    {
      id: 'codex',
      plugin_id: 'builtin-codex',
      display_name: 'Codex',
      enabled,
      source: 'builtin',
      icon_url: null
    }
  ]
}

export function renderRequestDetail(options: RequestDetailRenderOptions) {
  if (!options.element) {
    return
  }
  const t = translations[options.language]
  const detail = options.state?.detail
  const session = options.state?.session
  if (!options.state || !isBlockingStatus(options.state.status) || !detail) {
    options.element.hidden = true
    options.element.innerHTML = ''
    return
  }

  // 当前请求详情来自后端主状态 detail，避免前端从最近事件中猜测。
  const content = detail.error_message || detail.content || detail.summary
  options.element.hidden = false
  options.element.innerHTML = `
    <dt>${escapeHtml(t.project)}</dt>
    <dd>${escapeHtml(session?.project_name || t.none)}</dd>
    <dt>${escapeHtml(t.path)}</dt>
    <dd>${escapeHtml(session?.project_path || t.none)}</dd>
    <dt>${escapeHtml(t.toolLabel)}</dt>
    <dd>${escapeHtml(session ? translateTool(options.language, session.tool) : t.none)}</dd>
    <dt>${escapeHtml(t.requestContent)}</dt>
    <dd>${escapeHtml(content)}</dd>
    <dt>${escapeHtml(t.requestTime)}</dt>
    <dd>${escapeHtml(formatLocalTime(options.state.updated_at, options.language))}</dd>
  `
}

export function renderEvents(
  element: HTMLOListElement | null,
  events: NiumaEvent[],
  language: LanguageCode
) {
  const t = translations[language]
  if (!element) {
    return
  }
  if (events.length === 0) {
    element.innerHTML = `<li class="empty">${escapeHtml(t.noEvents)}</li>`
    return
  }
  element.innerHTML = events
    .map(
      (event) => `
        <li>
          <div class="event-row">
            <strong>${escapeHtml(translateEventType(language, event.event_type))}</strong>
            <span>${escapeHtml(translateTool(language, event.tool))}</span>
          </div>
          <p>${escapeHtml(event.summary)}</p>
          <small>${escapeHtml(event.project_name)} · ${escapeHtml(formatLocalTime(event.created_at, language))}</small>
        </li>
      `
    )
    .join('')
}

export function renderSessions(options: SessionRenderOptions) {
  const t = translations[options.language]
  const selectedSessionId = chooseSelectedSessionId(
    options.selectedSessionId,
    options.sessions,
    options.primarySessionId
  )
  if (!options.listElement || !options.overviewElement) {
    return selectedSessionId
  }
  if (options.sessions.length === 0) {
    options.listElement.innerHTML = `<p class="empty">${escapeHtml(t.noSessions)}</p>`
    options.overviewElement.innerHTML = `<div class="empty">${escapeHtml(t.noSessionSelected)}</div>`
    return selectedSessionId
  }
  options.listElement.innerHTML = sortedSessionsByLatestActivity(options.sessions)
    .map((session) => {
      const selected = session.id === selectedSessionId
      return `
        <button class="session-item${selected ? ' selected' : ''}" type="button" data-session-id="${escapeHtml(session.id)}">
          <strong>${escapeHtml(session.project_name || t.none)}</strong>
          <span>${escapeHtml(session.project_path || t.none)}</span>
          <small>${escapeHtml(translateTool(options.language, session.tool))} · ${escapeHtml(translateStatus(options.language, session.status))} · ${escapeHtml(formatLocalTime(session.last_activity_at, options.language))}</small>
        </button>
      `
    })
    .join('')
  renderSelectedSessionOverview(options, selectedSessionId)
  return selectedSessionId
}

function chooseSelectedSessionId(
  currentId: string | null,
  sessions: NiumaSession[],
  primarySessionId: string | null | undefined
) {
  if (currentId && sessions.some((session) => session.id === currentId)) {
    return currentId
  }
  if (primarySessionId && sessions.some((session) => session.id === primarySessionId)) {
    return primarySessionId
  }
  return sortedSessionsByLatestActivity(sessions)[0]?.id ?? null
}

function latestEventForSession(sessionId: string, events: NiumaEvent[]) {
  return events.find((event) => event.session_id === sessionId) ?? null
}

function sortedSessionsByLatestActivity(sessions: NiumaSession[]) {
  return [...sessions].sort(
    (left, right) =>
      new Date(right.last_activity_at).getTime() - new Date(left.last_activity_at).getTime()
  )
}

function renderSelectedSessionOverview(
  options: SessionRenderOptions,
  selectedSessionId: string | null
) {
  const t = translations[options.language]
  const session = options.sessions.find((item) => item.id === selectedSessionId)
  if (!options.overviewElement) {
    return
  }
  if (!session) {
    options.overviewElement.innerHTML = `<div class="empty">${escapeHtml(t.noSessionSelected)}</div>`
    return
  }
  const latestEvent = latestEventForSession(session.id, options.events)
  options.overviewElement.innerHTML = `
    <dt>${escapeHtml(t.projectName)}</dt>
    <dd>${escapeHtml(session.project_name || t.none)}</dd>
    <dt>${escapeHtml(t.path)}</dt>
    <dd>${escapeHtml(session.project_path || t.none)}</dd>
    <dt>${escapeHtml(t.toolLabel)}</dt>
    <dd>${escapeHtml(translateTool(options.language, session.tool))}</dd>
    <dt>${escapeHtml(t.sessionId)}</dt>
    <dd>${escapeHtml(session.id)}</dd>
    <dt>${escapeHtml(t.currentStatus)}</dt>
    <dd>${escapeHtml(translateStatus(options.language, session.status))}</dd>
    <dt>${escapeHtml(t.lastActivity)}</dt>
    <dd>${escapeHtml(formatLocalTime(session.last_activity_at, options.language))}</dd>
    <dt>${escapeHtml(t.latestEvent)}</dt>
    <dd>${escapeHtml(latestEvent ? latestEvent.summary : t.none)}</dd>
  `
}
