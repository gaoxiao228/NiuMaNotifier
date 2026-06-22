import {
  renderRequestDetail,
  renderSessions,
  renderStatusSummary,
  shouldShowManualBlockerAction
} from '../src/statusView'
import type { MainStatePayload, RuntimeStateItem } from '../src/api'

type RuntimeStateHasNoLegacyId = 'id' extends keyof RuntimeStateItem ? false : true

class FakeElement {
  className = ''
  innerHTML = ''
}

class FakeDetailElement extends FakeElement {
  hidden = false
}

class FakeActionElement extends FakeElement {
  hidden = false
}

const runtimeStates: RuntimeStateItem[] = [
  {
    tool: 'codex',
    session_id: 'runtime-session-new',
    project_path: '/Users/niuma/code/new',
    project_name: 'new-project',
    status: 'running',
    last_event_id: 'event-new',
    last_activity_at: '2026-06-16T10:00:00Z'
  },
  {
    tool: 'codex',
    session_id: 'runtime-session-old',
    project_path: '/Users/niuma/code/old',
    project_name: 'old-project',
    status: 'completed',
    last_event_id: null,
    last_activity_at: '2026-06-15T10:00:00Z'
  }
]

const runtimeStateWithoutLegacyId = runtimeStates[0] as RuntimeStateItem & { id?: never }
const runtimeStateHasNoLegacyId: RuntimeStateHasNoLegacyId = true

if ('id' in runtimeStateWithoutLegacyId) {
  throw new Error('运行态类型不应包含旧 id 字段')
}

if (!runtimeStateHasNoLegacyId) {
  throw new Error('运行态类型不应声明旧 id 字段')
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
    failure_reason: null,
    approval: null
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

const runtimeStateListElement = new FakeElement()
const runtimeStateOverviewElement = new FakeElement()
const selectedRuntimeStateId = renderSessions({
  listElement: runtimeStateListElement as HTMLElement,
  overviewElement: runtimeStateOverviewElement as HTMLDListElement,
  runtimeStates,
  events: [
    {
      id: 'event-old',
      tool: 'codex',
      session_id: 'runtime-session-old',
      project_name: 'old-project',
      project_path: '/Users/niuma/code/old',
      event_type: 'completed',
      severity: 'info',
      summary: '旧运行态事件',
      created_at: '2026-06-15T10:01:00Z'
    }
  ],
  selectedSessionId: 'runtime-session-old',
  primarySessionId: 'missing-session',
  language: 'zh-CN'
})

if (selectedRuntimeStateId !== 'runtime-session-old') {
  throw new Error('运行态列表应使用 session_id 保留当前选中项')
}

if (!runtimeStateListElement.innerHTML.includes('data-session-id="runtime-session-old"')) {
  throw new Error('运行态按钮应使用 session_id 写入 data-session-id')
}

if (!runtimeStateListElement.innerHTML.includes('session-item selected')) {
  throw new Error('运行态列表应根据 session_id 标记选中项')
}

if (!runtimeStateOverviewElement.innerHTML.includes('runtime-session-old')) {
  throw new Error('运行态详情应显示 session_id')
}

if (!runtimeStateOverviewElement.innerHTML.includes('旧运行态事件')) {
  throw new Error('运行态详情应使用 session_id 匹配最近事件')
}

const requestDetailElement = new FakeDetailElement()
const requestActionsElement = new FakeActionElement()
renderRequestDetail({
  element: requestDetailElement as HTMLDListElement,
  actionsElement: requestActionsElement as HTMLElement,
  state: {
    ...baseState,
    session: {
      id: 'session-should-not-render',
      tool: 'codex',
      project_path: '/Users/niuma/code/niuma-workspace/NiuMaNotifier',
      project_name: 'NiuMaNotifier'
    }
  },
  language: 'zh-CN',
  approving: false
})

if (requestDetailElement.innerHTML.includes('Session ID')) {
  throw new Error('主状态请求详情不应继续显示 Session ID 标签')
}

if (requestDetailElement.innerHTML.includes('session-should-not-render')) {
  throw new Error('主状态请求详情不应继续显示 Session ID 值')
}

const pendingApprovalActions = new FakeActionElement()
renderRequestDetail({
  element: new FakeDetailElement() as HTMLDListElement,
  actionsElement: pendingApprovalActions as HTMLElement,
  state: {
    ...baseState,
    detail: {
      ...baseState.detail!,
      approval: {
        request_id: 'approval-1',
        status: 'pending',
        can_decide: true,
        message: null,
        decided_by: null,
        decided_source: null
      }
    }
  },
  language: 'zh-CN',
  approving: false
})

if (!pendingApprovalActions.innerHTML.includes('同意')) {
  throw new Error('pending 授权应显示同意按钮')
}

if (!pendingApprovalActions.innerHTML.includes('拒绝')) {
  throw new Error('pending 授权应显示拒绝按钮')
}

const returnedApprovalActions = new FakeActionElement()
renderRequestDetail({
  element: new FakeDetailElement() as HTMLDListElement,
  actionsElement: returnedApprovalActions as HTMLElement,
  state: {
    ...baseState,
    detail: {
      ...baseState.detail!,
      approval: {
        request_id: 'approval-1',
        status: 'returned_to_codex',
        can_decide: false,
        message: 'Niuma 已停止代处理，请回到 Codex 中同意或拒绝',
        decided_by: 'hook-helper',
        decided_source: 'timeout'
      }
    }
  },
  language: 'zh-CN',
  approving: false
})

if (!returnedApprovalActions.innerHTML.includes('请回到 Codex')) {
  throw new Error('returned_to_codex 应显示回到 Codex 操作提示')
}

if (
  returnedApprovalActions.innerHTML.includes('data-approval-decision="allow"') ||
  returnedApprovalActions.innerHTML.includes('data-approval-decision="deny"')
) {
  throw new Error('returned_to_codex 不应继续显示同意或拒绝按钮')
}

if (
  shouldShowManualBlockerAction({
    ...baseState,
    detail: {
      ...baseState.detail!,
      approval: {
        request_id: 'approval-1',
        status: 'pending',
        can_decide: true,
        message: null,
        decided_by: null,
        decided_source: null
      }
    }
  })
) {
  throw new Error('hook 授权待决策时不应显示“我已处理”')
}

if (
  shouldShowManualBlockerAction({
    ...baseState,
    detail: {
      ...baseState.detail!,
      approval: {
        request_id: 'approval-1',
        status: 'allowed',
        can_decide: false,
        message: '已同意，等待 Codex 继续',
        decided_by: 'dashboard',
        decided_source: 'ui'
      }
    }
  })
) {
  throw new Error('hook 授权已决策但未完成时不应显示“我已处理”')
}

if (
  !shouldShowManualBlockerAction({
    ...baseState,
    detail: {
      ...baseState.detail!,
      approval: {
        request_id: 'approval-1',
        status: 'returned_to_codex',
        can_decide: false,
        message: 'Niuma 已停止代处理，请回到 Codex 中同意或拒绝',
        decided_by: 'hook-helper',
        decided_source: 'timeout'
      }
    }
  })
) {
  throw new Error('授权退回 Codex 后应允许显示“我已处理”')
}
