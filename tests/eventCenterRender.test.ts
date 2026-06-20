import type { NiumaEvent } from '../src/api'
import { renderEventCenter, setEventCenterItemExpanded } from '../src/eventCenterView'

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

if (!element.innerHTML.includes('class="event-center-detail"')) {
  throw new Error('展开事件时应在当前事件行下方渲染带动画的详情容器')
}

if (!element.innerHTML.includes('class="event-center-detail-inner"')) {
  throw new Error('事件详情应使用稳定内层容器支持展开和收缩动画')
}

if (!element.innerHTML.includes('aria-hidden="true"') || !element.innerHTML.includes('aria-hidden="false"')) {
  throw new Error('每条事件都应渲染稳定详情容器，并通过 aria-hidden 标识展开状态')
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
let itemExpanded = false
let toggleAriaExpanded = ''
let detailAriaHidden = ''
const detailElement = {
  setAttribute: (name: string, value: string) => {
    if (name === 'aria-hidden') {
      detailAriaHidden = value
    }
  }
}
const itemElement = {
  classList: {
    toggle: (className: string, force?: boolean) => {
      if (className === 'expanded') {
        itemExpanded = force === true
      }
    }
  },
  querySelector: (selector: string) => (selector === '.event-center-detail' ? detailElement : null)
}
const toggleElement = {
  setAttribute: (name: string, value: string) => {
    if (name === 'aria-expanded') {
      toggleAriaExpanded = value
    }
  },
  closest: (selector: string) => (selector === '.event-center-item' ? itemElement : null)
}
const rootElement = {
  querySelector: (selector: string) => {
    queriedSelector = selector
    return toggleElement
  }
}

setEventCenterItemExpanded(rootElement as unknown as HTMLElement, 'event-b', true)

if (queriedSelector !== '[data-event-center-toggle="event-b"]') {
  throw new Error('事件中心应按事件 id 局部查找当前事件行')
}

if (!itemExpanded || toggleAriaExpanded !== 'true' || detailAriaHidden !== 'false') {
  throw new Error('展开事件时应只局部更新当前事件项 class 和 aria 状态')
}
