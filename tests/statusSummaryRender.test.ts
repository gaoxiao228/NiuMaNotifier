import {
  blockerActionLabel,
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
  } as unknown as MainStatePayload,
  language: 'zh-CN',
  approving: false
})

if (!pendingApprovalActions.hidden || pendingApprovalActions.innerHTML.includes('同意')) {
  throw new Error('旧 approval 字段不应再渲染授权按钮')
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
  } as unknown as MainStatePayload,
  language: 'zh-CN',
  approving: false
})

if (!returnedApprovalActions.hidden || returnedApprovalActions.innerHTML.includes('请回到 Codex')) {
  throw new Error('旧 approval 字段不应再渲染回到工具处理提示')
}

const toolInteractionDetail = new FakeDetailElement()
const toolInteractionActions = new FakeActionElement()
const toolInteractionState: MainStatePayload = {
  ...baseState,
  session: {
    id: 'session-codex',
    tool: 'codex',
    project_path: '/Users/niuma/code/NiuMaNotifier',
    project_name: 'NiuMaNotifier'
  },
  detail: {
    ...baseState.detail!,
    interaction: {
      kind: 'approval',
      handling: 'tool',
      actionable: false,
      message: '请回到 Codex 中同意或拒绝'
    }
  }
}
renderRequestDetail({
  element: toolInteractionDetail as HTMLDListElement,
  actionsElement: toolInteractionActions as HTMLElement,
  state: toolInteractionState,
  language: 'zh-CN',
  approving: false
})

if (!toolInteractionDetail.innerHTML.includes('处理提示')) {
  throw new Error('不可操作 interaction 应在详情中显示处理提示标签')
}

if (!toolInteractionActions.innerHTML.includes('请回到 Codex 中同意或拒绝')) {
  throw new Error('不可操作 interaction 应在操作区显示回到工具处理提示')
}

if (blockerActionLabel(toolInteractionState, 'zh-CN') !== '我已在 Codex 中处理') {
  throw new Error('回工具处理的阻塞项应使用更明确的手动清理按钮文案')
}

if (!shouldShowManualBlockerAction(toolInteractionState)) {
  throw new Error('只能回工具处理的阻塞项应保留手动清理入口')
}

const niumaInteractionActions = new FakeActionElement()
const niumaInteractionState: MainStatePayload = {
  ...baseState,
  detail: {
    ...baseState.detail!,
    interaction: {
      kind: 'approval',
      handling: 'niuma',
      actionable: true,
      request_id: 'approval-from-interaction',
      actions: ['allow', 'deny'],
      endpoint: '/api/v1/approval-decisions'
    }
  }
}
renderRequestDetail({
  element: new FakeDetailElement() as HTMLDListElement,
  actionsElement: niumaInteractionActions as HTMLElement,
  state: niumaInteractionState,
  language: 'zh-CN',
  approving: false
})

if (!niumaInteractionActions.innerHTML.includes('data-approval-request-id="approval-from-interaction"')) {
  throw new Error('可操作 interaction 应使用 interaction.request_id 渲染授权按钮')
}

if (shouldShowManualBlockerAction(niumaInteractionState)) {
  throw new Error('可由 Niuma 处理的授权不应显示手动清理入口')
}

const inputInteractionDetail = new FakeDetailElement()
const inputInteractionActions = new FakeActionElement()
renderRequestDetail({
  element: inputInteractionDetail as HTMLDListElement,
  actionsElement: inputInteractionActions as HTMLElement,
  state: {
    ...baseState,
    status: 'waiting_input',
    detail: {
      ...baseState.detail!,
      summary: 'Codex 等待输入：请选择处理方式',
      content: '请选择处理方式\n1. 继续 - 继续当前任务\n2. 停止',
      interaction: {
        kind: 'input',
        handling: 'niuma',
        actionable: true,
        request_id: 'codex-input:niuma_codex_wrapper_1:request-1',
        schema: {
          questions: [
            {
              id: 'decision',
              question: '请选择处理方式',
              options: [
                {
                  label: '继续',
                  description: '继续当前任务'
                },
                {
                  label: '停止'
                }
              ]
            },
            {
              id: 'comment',
              question: '补充说明'
            }
          ]
        }
      }
    }
  },
  language: 'zh-CN',
  approving: false
})

if (!inputInteractionDetail.innerHTML.includes('Codex 等待输入：请选择处理方式')) {
  throw new Error('可操作等待输入详情应显示摘要')
}

if (inputInteractionDetail.innerHTML.includes('1. 继续 - 继续当前任务')) {
  throw new Error('可操作等待输入详情不应重复显示完整选项内容')
}

if (
  !inputInteractionActions.innerHTML.includes(
    'form data-input-request-id="codex-input:niuma_codex_wrapper_1:request-1"'
  )
) {
  throw new Error('renders actionable input form from interaction schema: 应渲染 input form')
}

if (!inputInteractionActions.innerHTML.includes('请选择处理方式')) {
  throw new Error('renders actionable input form from interaction schema: 应显示问题文案')
}

if (
  !inputInteractionActions.innerHTML.includes('type="radio"') ||
  !inputInteractionActions.innerHTML.includes('value="继续"') ||
  !inputInteractionActions.innerHTML.includes('继续当前任务')
) {
  throw new Error('renders actionable input form from interaction schema: 应显示选项题')
}

if (
  !inputInteractionActions.innerHTML.includes('自定义答案') ||
  !inputInteractionActions.innerHTML.includes('data-custom-input-for="decision"')
) {
  throw new Error('有选项的问题末尾应追加自定义答案输入')
}

if (
  !inputInteractionActions.innerHTML.includes('textarea') ||
  !inputInteractionActions.innerHTML.includes('placeholder="请输入回复"')
) {
  throw new Error('renders actionable input form from interaction schema: 无选项题应显示 textarea')
}

if (
  returnedApprovalActions.innerHTML.includes('data-approval-decision="allow"') ||
  returnedApprovalActions.innerHTML.includes('data-approval-decision="deny"')
) {
  throw new Error('returned_to_codex 不应继续显示同意或拒绝按钮')
}

if (
  !shouldShowManualBlockerAction({
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
  } as unknown as MainStatePayload)
) {
  throw new Error('旧 approval 字段不应隐藏手动清理入口')
}

if (
  !shouldShowManualBlockerAction({
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
  } as unknown as MainStatePayload)
) {
  throw new Error('没有 interaction 时应按普通阻塞项显示手动清理入口')
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
  } as unknown as MainStatePayload)
) {
  throw new Error('旧 approval 字段为 returned_to_codex 时仍只按普通阻塞项处理')
}
