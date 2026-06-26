export type SessionControlActionType = 'send_instruction' | 'interrupt'

export type SessionControlAction = {
  type: SessionControlActionType | string
  transport?: string | null
  endpoint?: string | null
  debug_command?: string | null
}

export type SessionControl = {
  available: boolean
  provider?: string | null
  wrapper_session_id?: string | null
  capabilities: string[]
  actions: SessionControlAction[]
}

export type SessionControlState = {
  canSendInstruction: boolean
  sendInstructionEndpoint: string | null
  showInterrupt: boolean
  canInterrupt: boolean
  interruptEndpoint: string | null
  disabledReason: 'control_unavailable' | 'send_instruction_unsupported' | null
}

export type SendInstructionPayload = {
  tool: string
  session_id: string
  wrapper_session_id: string
  content: string
}

export type InterruptPayload = {
  tool: string
  session_id: string
  wrapper_session_id: string
}

export function findControlAction(
  control: SessionControl | null | undefined,
  actionType: SessionControlActionType
) {
  // 控制区当前只调用本地 API action，避免误用 debug_command 等非接口动作。
  return (
    control?.actions.find(
      (action) =>
        action.type === actionType &&
        action.transport === 'local_api' &&
        typeof action.endpoint === 'string' &&
        action.endpoint.length > 0
    ) ?? null
  )
}

export function getSessionControlState(
  control: SessionControl | null | undefined,
  listRuntimeStatus: string | null | undefined
): SessionControlState {
  const sendAction = findControlAction(control, 'send_instruction')
  const interruptAction = findControlAction(control, 'interrupt')
  const available = control?.available === true
  const supportsSend =
    available && control.capabilities.includes('send_instruction') && Boolean(sendAction)
  const supportsInterrupt =
    available && control.capabilities.includes('interrupt') && Boolean(interruptAction)

  return {
    canSendInstruction: supportsSend,
    sendInstructionEndpoint: sendAction?.endpoint ?? null,
    showInterrupt: supportsInterrupt,
    canInterrupt: supportsInterrupt && listRuntimeStatus === 'running',
    interruptEndpoint: interruptAction?.endpoint ?? null,
    disabledReason: supportsSend
      ? null
      : available
        ? 'send_instruction_unsupported'
        : 'control_unavailable'
  }
}

export function buildSendInstructionPayload(input: {
  tool: string
  sessionId: string
  wrapperSessionId: string
  content: string
}): SendInstructionPayload {
  return {
    tool: input.tool,
    session_id: input.sessionId,
    wrapper_session_id: input.wrapperSessionId,
    content: input.content
  }
}

export function buildInterruptPayload(input: {
  tool: string
  sessionId: string
  wrapperSessionId: string
}): InterruptPayload {
  return {
    tool: input.tool,
    session_id: input.sessionId,
    wrapper_session_id: input.wrapperSessionId
  }
}
