import { renderRequestDetail, renderStatusSummary } from '../src/statusView'
import type { MainStatePayload } from '../src/api'

class FakeElement {
  className = ''
  innerHTML = ''
}

class FakeDetailElement extends FakeElement {
  hidden = false
}

function stateWithStatus(status: string): MainStatePayload {
  return {
    ...baseState,
    status
  }
}

function renderStatus(status: string) {
  const element = new FakeElement()
  renderStatusSummary({
    element: element as HTMLElement,
    state: stateWithStatus(status),
    language: 'zh-CN'
  })
  return element
}

const baseState: MainStatePayload = {
  version: 1,
  status: 'waiting_approval',
  updated_at: '2026-06-16T10:00:00Z',
  session: null,
  detail: {
    event_id: 'event-1',
    event_type: 'approval_requested',
    severity: 'warning',
    summary: '需要处理',
    content: 'Codex 请求批准',
    error_message: null,
    payload_ref: null,
    completion_reason: null,
    failure_reason: null
  }
}

const waitingApprovalElement = renderStatus('waiting_approval')

if (!waitingApprovalElement.innerHTML.includes('等待批准')) {
  throw new Error('主状态应显示当前状态')
}

if (
  waitingApprovalElement.innerHTML.includes('等待输入') ||
  waitingApprovalElement.innerHTML.includes('正在运行')
) {
  throw new Error('主状态不应显示其他状态')
}

if (!waitingApprovalElement.className.includes('danger')) {
  throw new Error('等待批准应使用红色状态分组')
}

if (!waitingApprovalElement.innerHTML.includes('<span class="status-icon" aria-hidden="true"></span>')) {
  throw new Error('主状态图标应为空的实心圆点，不应包含字符')
}

for (const status of ['waiting_input', 'error']) {
  const element = renderStatus(status)
  if (!element.className.includes('danger')) {
    throw new Error(`${status} 应使用红色状态分组`)
  }
}

for (const status of ['completed', 'idle']) {
  const element = renderStatus(status)
  if (!element.className.includes('info')) {
    throw new Error(`${status} 应使用绿色圆点状态分组`)
  }
}

const runningElement = renderStatus('running')
if (!runningElement.className.includes('warning')) {
  throw new Error('running 应使用黄色圆点状态分组')
}

if (runningElement.innerHTML.includes('<p>') || runningElement.innerHTML.includes('running')) {
  throw new Error('主状态摘要不应在状态标题下方重复显示 raw status')
}

const requestDetailElement = new FakeDetailElement()
renderRequestDetail({
  element: requestDetailElement as HTMLDListElement,
  state: {
    ...baseState,
    session: {
      id: 'session-should-not-render',
      tool: 'codex',
      project_path: '/Users/niuma/code/niuma-workspace/NiuMaNotifier',
      project_name: 'NiuMaNotifier'
    }
  },
  language: 'zh-CN'
})

if (requestDetailElement.innerHTML.includes('Session ID')) {
  throw new Error('主状态请求详情不应继续显示 Session ID 标签')
}

if (requestDetailElement.innerHTML.includes('session-should-not-render')) {
  throw new Error('主状态请求详情不应继续显示 Session ID 值')
}
