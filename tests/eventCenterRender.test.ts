import type { NiumaEvent } from '../src/api'
import { renderEventCenter, scrollEventCenterItemIntoView } from '../src/eventCenterView'

class FakeElement {
  // 测试只需要验证 renderer 写入的 HTML 字符串。
  innerHTML = ''
}

const eventA = {
  id: 'event-a',
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
} satisfies NiumaEvent

const eventB = {
  id: 'event-b',
  tool: 'codex',
  session_id: 'session-b',
  project_name: 'NiuMaNotifier',
  project_path: '/repo/NiuMaNotifier',
  event_type: 'task_failed',
  severity: 'urgent',
  summary: '<script>alert(1)</script>',
  content: null,
  error_message: 'assertion failed',
  created_at: '2026-06-20T10:01:00Z'
} satisfies NiumaEvent

const element = new FakeElement()

renderEventCenter({
  element: element as HTMLElement,
  language: 'zh-CN',
  events: [],
  expandedEventIds: new Set(),
  connected: false,
  connecting: true,
  errorText: ''
})

if (!element.innerHTML.includes('等待新的实时事件')) {
  throw new Error('事件中心空态应提示等待实时事件')
}

if (!element.innerHTML.includes('实时连接中')) {
  throw new Error('事件中心应展示连接中状态')
}

renderEventCenter({
  element: element as HTMLElement,
  language: 'zh-CN',
  events: [eventA, eventB],
  expandedEventIds: new Set(['event-b']),
  connected: true,
  connecting: false,
  errorText: ''
})

const firstIndex = element.innerHTML.indexOf('Bash: npm test')
const secondIndex = element.innerHTML.indexOf('&lt;script&gt;alert(1)&lt;/script&gt;')

if (firstIndex < 0 || secondIndex < 0 || firstIndex > secondIndex) {
  throw new Error('事件中心应按追加顺序渲染，新的实时事件出现在底部')
}

if (!element.innerHTML.includes('data-event-center-toggle="event-b"')) {
  throw new Error('事件项应提供点击展开目标')
}

if (!element.innerHTML.includes('class="event-center-json"')) {
  throw new Error('展开事件时应显示格式化 JSON 详情')
}

if (!element.innerHTML.includes('&quot;id&quot;: &quot;event-b&quot;')) {
  throw new Error('格式化 JSON 应经过 HTML 转义，避免原始事件内容注入页面')
}

if (!element.innerHTML.includes('实时已连接')) {
  throw new Error('事件中心应展示实时已连接状态')
}

renderEventCenter({
  element: element as HTMLElement,
  language: 'zh-CN',
  events: [eventA],
  expandedEventIds: new Set(),
  connected: false,
  connecting: false,
  errorText: '连接失败'
})

if (!element.innerHTML.includes('实时已断开') || !element.innerHTML.includes('连接失败')) {
  throw new Error('事件中心断开时应显示断开状态和错误文案')
}

let queriedSelector = ''
let closestSelector = ''
const listElement = {
  scrollTop: 24,
  getBoundingClientRect: () => ({ top: 100, bottom: 500 })
}
const visibleItemElement = {
  closest: (selector: string) => (selector === '.event-center-list' ? listElement : null),
  getBoundingClientRect: () => ({ top: 130, bottom: 320 })
}
const toggleElement = {
  closest: (selector: string) => {
    closestSelector = selector
    return visibleItemElement
  }
}
const rootElement = {
  querySelector: (selector: string) => {
    queriedSelector = selector
    return toggleElement
  }
}

scrollEventCenterItemIntoView(rootElement as unknown as HTMLElement, 'event-b')

if (queriedSelector !== '[data-event-center-toggle="event-b"]') {
  throw new Error('事件中心应按事件 id 查找刚展开的事件行')
}

if (closestSelector !== '.event-center-item') {
  throw new Error('事件中心应滚动整个事件项，确保展开详情也进入可视区域')
}

if (listElement.scrollTop !== 24) {
  throw new Error('展开已完整可见的事件项时不应改变当前滚动位置')
}

const overflowListElement = {
  scrollTop: 40,
  getBoundingClientRect: () => ({ top: 100, bottom: 500 })
}
const overflowItemElement = {
  closest: (selector: string) => (selector === '.event-center-list' ? overflowListElement : null),
  getBoundingClientRect: () => ({ top: 420, bottom: 560 })
}
const overflowRootElement = {
  querySelector: () => ({
    closest: () => overflowItemElement
  })
}

scrollEventCenterItemIntoView(overflowRootElement as unknown as HTMLElement, 'event-b')

if (overflowListElement.scrollTop !== 100) {
  throw new Error('展开详情超出列表底部时，应只按溢出高度补充滚动')
}
