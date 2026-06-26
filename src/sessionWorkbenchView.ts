import type { InterruptSessionResult, SendInstructionResult, ToolSessionDetail } from './api'
import {
  buildInterruptPayload,
  buildSendInstructionPayload,
  getSessionControlState,
  type InterruptPayload,
  type SendInstructionPayload
} from './sessionControl'
import { escapeHtml } from './viewUtils'

export type SessionDetailControlText = {
  placeholder: string
  send: string
  interrupt: string
  unsupported: string
}

export type SessionDetailControlRenderOptions = {
  detail: ToolSessionDetail
  listRuntimeStatus?: string | null
  text: SessionDetailControlText
  busy: boolean
  error?: string | null
}

export type BindSessionDetailControlOptions = {
  detail: ToolSessionDetail
  listRuntimeStatus?: string | null
  rerender: (error: string | null, busy: boolean) => void | Promise<void>
  sendInstruction?: (
    endpoint: string,
    payload: SendInstructionPayload
  ) => Promise<SendInstructionResult>
  interruptSession?: (
    endpoint: string,
    payload: InterruptPayload
  ) => Promise<InterruptSessionResult>
}

function getErrorMessage(error: unknown) {
  // DOM 事件内只展示可读错误，避免把非 Error 对象直接拼进页面。
  return error instanceof Error ? error.message : String(error)
}

export function renderSessionDetailControl(options: SessionDetailControlRenderOptions) {
  const state = getSessionControlState(options.detail.control, options.listRuntimeStatus)
  const inputDisabled = options.busy || !state.canSendInstruction
  const sendDisabled = options.busy || !state.canSendInstruction
  const interruptDisabled = options.busy || !state.canInterrupt
  const message = options.error ?? (state.canSendInstruction ? '' : options.text.unsupported)

  return `
    <section class="session-detail-control">
      <textarea
        data-session-control-input
        placeholder="${escapeHtml(options.text.placeholder)}"
        ${inputDisabled ? 'disabled' : ''}
      ></textarea>
      <div class="session-detail-control-actions">
        <button type="button" data-session-action="send" ${sendDisabled ? 'disabled' : ''}>
          ${escapeHtml(options.text.send)}
        </button>
        ${
          state.showInterrupt
            ? `<button type="button" data-session-action="interrupt" ${
                interruptDisabled ? 'disabled' : ''
              }>${escapeHtml(options.text.interrupt)}</button>`
            : ''
        }
      </div>
      <p data-session-control-message ${message ? '' : 'hidden'}>
        ${message ? escapeHtml(message) : ''}
      </p>
    </section>
  `
}

export function bindSessionDetailControl(
  root: ParentNode,
  options: BindSessionDetailControlOptions
) {
  const input = root.querySelector<HTMLTextAreaElement>('[data-session-control-input]')
  const sendButton = root.querySelector<HTMLButtonElement>('[data-session-action="send"]')
  const interruptButton = root.querySelector<HTMLButtonElement>('[data-session-action="interrupt"]')
  const state = getSessionControlState(options.detail.control, options.listRuntimeStatus)
  const wrapperSessionId = options.detail.control?.wrapper_session_id ?? null
  let requestBusy = false

  sendButton?.addEventListener('click', async () => {
    const content = input?.value.trim() ?? ''
    if (
      requestBusy ||
      !content ||
      sendButton.disabled ||
      !state.canSendInstruction ||
      !state.sendInstructionEndpoint ||
      !wrapperSessionId
    ) {
      return
    }

    requestBusy = true
    try {
      await options.rerender(null, true)
      // API 模块依赖 Tauri runtime；延迟加载可让纯字符串渲染测试在 Node 中运行。
      const sendInstruction =
        options.sendInstruction ?? (await import('./api')).sendSessionInstruction
      await sendInstruction(
        state.sendInstructionEndpoint,
        buildSendInstructionPayload({
          tool: options.detail.tool,
          sessionId: options.detail.session_id,
          wrapperSessionId,
          content
        })
      )
      if (input) {
        input.value = ''
      }
      await options.rerender(null, false)
    } catch (error) {
      await options.rerender(getErrorMessage(error), false)
    } finally {
      requestBusy = false
    }
  })

  interruptButton?.addEventListener('click', async () => {
    if (
      requestBusy ||
      interruptButton.disabled ||
      !state.canInterrupt ||
      !state.interruptEndpoint ||
      !wrapperSessionId
    ) {
      return
    }

    requestBusy = true
    try {
      await options.rerender(null, true)
      // API 模块依赖 Tauri runtime；延迟加载可让纯字符串渲染测试在 Node 中运行。
      const interruptSession =
        options.interruptSession ?? (await import('./api')).interruptSession
      await interruptSession(
        state.interruptEndpoint,
        buildInterruptPayload({
          tool: options.detail.tool,
          sessionId: options.detail.session_id,
          wrapperSessionId
        })
      )
      await options.rerender(null, false)
    } catch (error) {
      await options.rerender(getErrorMessage(error), false)
    } finally {
      requestBusy = false
    }
  })
}
