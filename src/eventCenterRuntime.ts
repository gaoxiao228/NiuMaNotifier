import type { NiumaEvent } from './api'

export const eventStreamPath = '/api/v1/events/stream'

export type EventSourceLike = {
  onopen: (() => void) | null
  onerror: (() => void) | null
  addEventListener: (type: string, listener: (event: MessageEvent<string>) => void) => void
  close: () => void
}

export type EventCenterRuntimeSnapshot = {
  events: NiumaEvent[]
  expandedEventIds: Set<string>
  connected: boolean
  connecting: boolean
  errorText: string
}

export type EventCenterRuntimeDependencies = {
  getLocalApiUrl: () => Promise<string>
  createEventSource: (url: string) => EventSourceLike
  isActive: () => boolean
  onChange: () => void
  disconnectedText: () => string
}

export type EventCenterRuntime = {
  snapshot: () => EventCenterRuntimeSnapshot
  start: () => void
  stop: () => void
  toggle: (eventId: string) => boolean
}

const requiredStringFields = [
  'id',
  'tool',
  'session_id',
  'project_name',
  'project_path',
  'event_type',
  'severity',
  'summary',
  'created_at'
] as const

export function isNiumaEvent(value: unknown): value is NiumaEvent {
  if (!isRecord(value)) {
    return false
  }
  if (!requiredStringFields.every((field) => typeof value[field] === 'string')) {
    return false
  }
  return isOptionalText(value.content) && isOptionalText(value.error_message)
}

export function createEventCenterRuntime(deps: EventCenterRuntimeDependencies): EventCenterRuntime {
  let events: NiumaEvent[] = []
  let expandedEventIds = new Set<string>()
  let source: EventSourceLike | undefined
  let generation = 0
  let connected = false
  let connecting = false
  let errorText = ''

  function snapshot(): EventCenterRuntimeSnapshot {
    return {
      events: [...events],
      expandedEventIds: new Set(expandedEventIds),
      connected,
      connecting,
      errorText
    }
  }

  function start() {
    if (source || connecting || !deps.isActive()) {
      return
    }
    generation += 1
    events = []
    expandedEventIds = new Set()
    connected = false
    connecting = true
    errorText = ''
    deps.onChange()
    void connect(generation)
  }

  function stop() {
    generation += 1
    source?.close()
    source = undefined
    connected = false
    connecting = false
    errorText = ''
  }

  function toggle(eventId: string) {
    if (expandedEventIds.has(eventId)) {
      expandedEventIds.delete(eventId)
      return false
    } else {
      expandedEventIds.add(eventId)
      return true
    }
  }

  async function connect(streamGeneration: number) {
    try {
      const apiUrl = await deps.getLocalApiUrl()
      if (!isCurrent(streamGeneration)) {
        return
      }
      const nextSource = deps.createEventSource(`${apiUrl}${eventStreamPath}`)
      if (!isCurrent(streamGeneration)) {
        nextSource.close()
        return
      }
      source = nextSource
      nextSource.onopen = () => {
        if (!isCurrent(streamGeneration, nextSource)) {
          return
        }
        connected = true
        connecting = false
        errorText = ''
        deps.onChange()
      }
      nextSource.addEventListener('event', (message) => {
        if (!isCurrent(streamGeneration, nextSource)) {
          return
        }
        appendEvent(message.data)
      })
      nextSource.onerror = () => {
        if (!isCurrent(streamGeneration, nextSource)) {
          return
        }
        connected = false
        connecting = false
        errorText = deps.disconnectedText()
        deps.onChange()
      }
    } catch (error) {
      if (!isCurrent(streamGeneration)) {
        return
      }
      connected = false
      connecting = false
      errorText = error instanceof Error ? error.message : String(error)
      deps.onChange()
    }
  }

  function appendEvent(data: string) {
    try {
      const parsed: unknown = JSON.parse(data)
      if (!isNiumaEvent(parsed)) {
        throw new Error('Invalid event payload')
      }
      if (events.some((event) => event.id === parsed.id)) {
        if (errorText) {
          errorText = ''
          deps.onChange()
        }
        return
      }
      // 事件中心是实时观察窗口，新消息按到达顺序追加到底部。
      events = [...events, parsed]
      errorText = ''
      deps.onChange()
    } catch (error) {
      errorText = error instanceof Error ? error.message : String(error)
      deps.onChange()
    }
  }

  function isCurrent(streamGeneration: number, currentSource?: EventSourceLike) {
    if (streamGeneration !== generation || !deps.isActive()) {
      return false
    }
    return currentSource === undefined || source === currentSource
  }

  return {
    snapshot,
    start,
    stop,
    toggle
  }
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
}

function isOptionalText(value: unknown) {
  return value === undefined || value === null || typeof value === 'string'
}
