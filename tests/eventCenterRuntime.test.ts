import type { NiumaEvent } from '../src/api'
import { createEventCenterRuntime, type EventSourceLike } from '../src/eventCenterRuntime'

type EventListener = (event: MessageEvent<string>) => void

class FakeEventSource implements EventSourceLike {
  // 测试只模拟事件中心 runtime 实际依赖的 EventSource 行为。
  onopen: (() => void) | null = null
  onerror: (() => void) | null = null
  closed = false
  private readonly listeners = new Map<string, EventListener[]>()

  addEventListener(type: string, listener: EventListener) {
    this.listeners.set(type, [...(this.listeners.get(type) ?? []), listener])
  }

  close() {
    this.closed = true
  }

  emitOpen() {
    this.onopen?.()
  }

  emitError() {
    this.onerror?.()
  }

  emitEvent(data: string) {
    for (const listener of this.listeners.get('event') ?? []) {
      listener({ data } as MessageEvent<string>)
    }
  }
}

function validEvent(id = 'event-a'): NiumaEvent {
  return {
    id,
    tool: 'codex',
    session_id: 'session-a',
    project_name: 'NiuMaNotifier',
    project_path: '/repo/NiuMaNotifier',
    event_type: 'approval_requested',
    severity: 'urgent',
    summary: 'Bash: npm test',
    content: 'Run npm test',
    error_message: null,
    created_at: '2026-06-20T10:00:00Z'
  }
}

function createDeferred<T>() {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((innerResolve) => {
    resolve = innerResolve
  })
  return { promise, resolve }
}

async function flushPromises() {
  await Promise.resolve()
  await Promise.resolve()
}

function assert(condition: unknown, message: string): asserts condition {
  if (!condition) {
    throw new Error(message)
  }
}

function createHarness(options?: { active?: boolean; apiUrl?: Promise<string> | string }) {
  let active = options?.active ?? true
  let changes = 0
  const sources: FakeEventSource[] = []
  const runtime = createEventCenterRuntime({
    getLocalApiUrl: () => Promise.resolve(options?.apiUrl ?? 'http://127.0.0.1:4321'),
    createEventSource: (url: string) => {
      assert(url === 'http://127.0.0.1:4321/api/v1/events/stream', 'runtime 应连接实时事件 SSE 路径')
      const source = new FakeEventSource()
      sources.push(source)
      return source
    },
    isActive: () => active,
    onChange: () => {
      changes += 1
    },
    disconnectedText: () => '实时已断开'
  })
  return {
    runtime,
    sources,
    changes: () => changes,
    setActive: (nextActive: boolean) => {
      active = nextActive
    }
  }
}

async function rejectsMalformedEventPayloadsWithoutAppending() {
  const { runtime, sources } = createHarness()

  runtime.start()
  await flushPromises()
  sources[0].emitEvent(JSON.stringify({}))

  let snapshot = runtime.snapshot()
  assert(snapshot.events.length === 0, '错误结构的 payload 不应进入事件列表')
  assert(snapshot.errorText.length > 0, '错误结构的 payload 应设置错误文案')

  sources[0].emitEvent(JSON.stringify(validEvent()))
  snapshot = runtime.snapshot()
  assert(snapshot.events.length === 1, '合法 payload 应追加到事件列表')
  assert(snapshot.errorText === '', '合法 payload 应清除旧错误文案')
}

async function ignoresStaleCallbacksAfterStop() {
  const { runtime, sources } = createHarness()

  runtime.start()
  await flushPromises()
  const oldSource = sources[0]
  runtime.stop()
  oldSource.emitEvent(JSON.stringify(validEvent()))

  assert(runtime.snapshot().events.length === 0, 'stop 后旧 source 的回调应被忽略')
  assert(oldSource.closed, 'stop 应关闭当前事件流')
}

async function doesNotCreateDuplicatePendingStreams() {
  const apiUrl = createDeferred<string>()
  const { runtime, sources } = createHarness({ apiUrl: apiUrl.promise })

  runtime.start()
  runtime.start()
  apiUrl.resolve('http://127.0.0.1:4321')
  await flushPromises()

  assert(sources.length === 1, '连接 URL 未返回时重复 start 不应创建多个事件流')
}

async function dedupesEventsAndClearsStaleErrorsOnDuplicateValidPayload() {
  const { runtime, sources, changes } = createHarness()
  const event = validEvent('event-dedupe')

  runtime.start()
  await flushPromises()
  sources[0].emitEvent(JSON.stringify(event))
  sources[0].emitEvent('{')
  const changesBeforeDuplicate = changes()
  sources[0].emitEvent(JSON.stringify(event))

  const snapshot = runtime.snapshot()
  assert(snapshot.events.length === 1, '重复 id 的合法事件不应再次追加')
  assert(snapshot.errorText === '', '重复合法事件也应清除旧错误文案')
  assert(changes() > changesBeforeDuplicate, '重复合法事件清除旧错误时应触发重新渲染')
}

async function ignoresCallbacksWhenPanelIsNoLongerActive() {
  const { runtime, sources, setActive } = createHarness()

  runtime.start()
  await flushPromises()
  setActive(false)
  sources[0].emitEvent(JSON.stringify(validEvent('event-inactive')))

  assert(runtime.snapshot().events.length === 0, '事件中心失活后应忽略当前 source 的回调')
}

function toggleUpdatesExpandedIds() {
  const { runtime, changes } = createHarness()
  const changesBeforeToggle = changes()

  const expanded = runtime.toggle('event-a')
  let snapshot = runtime.snapshot()
  assert(expanded, 'toggle 应返回当前事件展开后的状态')
  assert(snapshot.expandedEventIds.has('event-a'), 'toggle 应展开指定事件')

  const collapsed = runtime.toggle('event-a')
  snapshot = runtime.snapshot()
  assert(!collapsed, '再次 toggle 应返回当前事件收起后的状态')
  assert(!snapshot.expandedEventIds.has('event-a'), '再次 toggle 应收起指定事件')
  assert(changes() === changesBeforeToggle, 'toggle 只应局部更新事件项，不应触发整列表重新渲染')
}

function doesNotStartWhenInactive() {
  const { runtime, sources } = createHarness({ active: false })

  runtime.start()

  assert(sources.length === 0, '事件中心非当前面板时不应创建事件流')
  assert(runtime.snapshot().events.length === 0, '非当前面板 start 不应加载历史事件')
}

async function run() {
  await rejectsMalformedEventPayloadsWithoutAppending()
  await ignoresStaleCallbacksAfterStop()
  await doesNotCreateDuplicatePendingStreams()
  await dedupesEventsAndClearsStaleErrorsOnDuplicateValidPayload()
  await ignoresCallbacksWhenPanelIsNoLongerActive()
  toggleUpdatesExpandedIds()
  doesNotStartWhenInactive()
}

void run()
