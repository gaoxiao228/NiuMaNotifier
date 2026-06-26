import {
  buildInterruptPayload,
  buildSendInstructionPayload,
  findControlAction,
  getSessionControlState,
  type SessionControl
} from '../src/sessionControl'

const control: SessionControl = {
  available: true,
  provider: 'niuma_codex',
  wrapper_session_id: 'niuma_codex_1',
  capabilities: ['send_instruction', 'interrupt'],
  actions: [
    {
      type: 'send_instruction',
      transport: 'local_api',
      endpoint: '/api/v1/session-control/send-instruction'
    },
    {
      type: 'interrupt',
      transport: 'local_api',
      endpoint: '/api/v1/session-control/interrupt'
    }
  ]
}

if (findControlAction(control, 'send_instruction')?.endpoint !== '/api/v1/session-control/send-instruction') {
  throw new Error('应找到 send_instruction local_api action')
}

const runningState = getSessionControlState(control, 'running')
if (!runningState.canSendInstruction) {
  throw new Error('支持 send_instruction 时发送应可用')
}
if (!runningState.showInterrupt || !runningState.canInterrupt) {
  throw new Error('running 且支持 interrupt 时中断应显示且可点击')
}

const idleState = getSessionControlState(control, 'idle')
if (!idleState.showInterrupt || idleState.canInterrupt) {
  throw new Error('非 running 时中断应显示但禁用')
}

const noInterruptState = getSessionControlState(
  { ...control, capabilities: ['send_instruction'], actions: control.actions.slice(0, 1) },
  'running'
)
if (noInterruptState.showInterrupt) {
  throw new Error('没有 interrupt 能力时中断按钮应隐藏')
}

const unavailableState = getSessionControlState(null, 'running')
if (unavailableState.canSendInstruction || unavailableState.showInterrupt) {
  throw new Error('没有 control 时只能显示禁用发送入口，不显示中断')
}

const sendPayload = buildSendInstructionPayload({
  tool: 'codex',
  sessionId: 'codex-session-1',
  wrapperSessionId: 'niuma_codex_1',
  content: '继续'
})
if (sendPayload.session_id !== 'codex-session-1' || sendPayload.content !== '继续') {
  throw new Error('发送 payload 应包含 session_id 和 content')
}

const interruptPayload = buildInterruptPayload({
  tool: 'codex',
  sessionId: 'codex-session-1',
  wrapperSessionId: 'niuma_codex_1'
})
if (interruptPayload.wrapper_session_id !== 'niuma_codex_1') {
  throw new Error('中断 payload 应包含 wrapper_session_id')
}
