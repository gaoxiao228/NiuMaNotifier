import {
  bindSessionDetailControl,
  renderSessionDetailControl,
  type BindSessionDetailControlOptions
} from '../src/sessionWorkbenchView'
import type { InterruptSessionResult, SendInstructionResult, ToolSessionDetail } from '../src/api'
import type { InterruptPayload, SendInstructionPayload } from '../src/sessionControl'

const text = {
  placeholder: '发送指令',
  send: '发送',
  interrupt: '中断',
  unsupported: '当前会话不支持发送指令'
}

const detail: ToolSessionDetail = {
  tool: 'codex',
  session_id: 'codex-session-1',
  messages: [],
  control: {
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
}

const enabledHtml = renderSessionDetailControl({
  detail,
  listRuntimeStatus: 'running',
  text,
  busy: false,
  error: null
})
if (!enabledHtml.includes('class="session-detail-control"')) {
  throw new Error('详情底部应始终渲染控制区')
}
if (!enabledHtml.includes('data-session-control-input')) {
  throw new Error('控制区应渲染可识别的输入框')
}
if (
  !enabledHtml.includes('data-session-action="send"') ||
  enabledHtml.includes('data-session-action="send" disabled')
) {
  throw new Error('支持 send_instruction 时发送按钮应可用')
}
if (
  !enabledHtml.includes('data-session-action="interrupt"') ||
  enabledHtml.includes('data-session-action="interrupt" disabled')
) {
  throw new Error('running 且支持 interrupt 时中断按钮应可用')
}

const idleHtml = renderSessionDetailControl({
  detail,
  listRuntimeStatus: 'idle',
  text,
  busy: false,
  error: null
})
if (
  !idleHtml.includes('data-session-action="interrupt"') ||
  !idleHtml.includes('data-session-action="interrupt" disabled')
) {
  throw new Error('非 running 时中断按钮应显示但禁用')
}

const noControlHtml = renderSessionDetailControl({
  detail: { ...detail, control: null },
  listRuntimeStatus: 'running',
  text,
  busy: false,
  error: null
})
if (
  !noControlHtml.includes('data-session-control-input') ||
  !noControlHtml.includes('data-session-control-input') ||
  !noControlHtml.includes('disabled')
) {
  throw new Error('没有 control 时输入框应禁用')
}
if (noControlHtml.includes('data-session-action="interrupt"')) {
  throw new Error('没有 interrupt 能力时中断按钮应隐藏')
}

const errorHtml = renderSessionDetailControl({
  detail,
  listRuntimeStatus: 'running',
  text,
  busy: false,
  error: '发送失败'
})
if (!errorHtml.includes('发送失败')) {
  throw new Error('错误文案应渲染到控制区')
}

const emptyMessageHtml = renderSessionDetailControl({
  detail,
  listRuntimeStatus: 'running',
  text,
  busy: false,
  error: null
})
if (!emptyMessageHtml.includes('data-session-control-message hidden')) {
  throw new Error('没有提示信息时 message 节点应隐藏')
}

const escapedHtml = renderSessionDetailControl({
  detail,
  listRuntimeStatus: 'running',
  text: { ...text, send: '<send>' },
  busy: false,
  error: '<script>失败</script>'
})
if (!escapedHtml.includes('&lt;send&gt;') || escapedHtml.includes('<script>')) {
  throw new Error('控制区用户可见文本和错误应进行 HTML 转义')
}

if (typeof bindSessionDetailControl !== 'function') {
  throw new Error('控制区应导出 DOM 事件绑定函数')
}

type ClickHandler = () => void | Promise<void>

class FakeButton {
  disabled = false
  private clickHandler: ClickHandler | null = null

  addEventListener(eventName: string, handler: ClickHandler) {
    if (eventName === 'click') {
      this.clickHandler = handler
    }
  }

  async click() {
    await this.clickHandler?.()
  }
}

class FakeRoot {
  readonly input = { value: '' }
  readonly sendButton = new FakeButton()
  readonly interruptButton = new FakeButton()

  querySelector(selector: string) {
    // 只实现绑定函数依赖的选择器，让 Node 环境测试不需要真实 document。
    if (selector === '[data-session-control-input]') {
      return this.input
    }
    if (selector === '[data-session-action="send"]') {
      return this.sendButton
    }
    if (selector === '[data-session-action="interrupt"]') {
      return this.interruptButton
    }
    return null
  }
}

async function verifyBindSessionDetailControl() {
  const root = new FakeRoot()
  const sentPayloads: unknown[] = []
  const interruptedPayloads: unknown[] = []
  const options: BindSessionDetailControlOptions = {
    detail,
    listRuntimeStatus: 'running',
    rerender: () => {},
    sendInstruction: async (
      _endpoint: string,
      payload: SendInstructionPayload
    ): Promise<SendInstructionResult> => {
      sentPayloads.push({ endpoint: _endpoint, payload })
      return { wrapper_session_id: 'niuma_codex_1', sent: true }
    },
    interruptSession: async (
      _endpoint: string,
      payload: InterruptPayload
    ): Promise<InterruptSessionResult> => {
      interruptedPayloads.push({ endpoint: _endpoint, payload })
      return { wrapper_session_id: 'niuma_codex_1', interrupted: true }
    }
  }

  bindSessionDetailControl(root as unknown as ParentNode, options)

  root.input.value = '  继续执行任务  '
  await root.sendButton.click()
  if (sentPayloads.length !== 1) {
    throw new Error('发送 click 应调用 sendInstruction')
  }
  const sendCall = sentPayloads[0] as {
    endpoint: string
    payload: { content: string }
  }
  if (
    sendCall.endpoint !== '/api/v1/session-control/send-instruction' ||
    sendCall.payload.content !== '继续执行任务'
  ) {
    throw new Error('发送 click 应 trim 输入并传入正确 endpoint 和 payload')
  }
  if (root.input.value !== '') {
    throw new Error('发送成功后应清空输入框')
  }

  await root.sendButton.click()
  if (sentPayloads.length !== 1) {
    throw new Error('空内容不应调用 sendInstruction')
  }

  await root.interruptButton.click()
  if (interruptedPayloads.length !== 1) {
    throw new Error('中断 click 应调用 interruptSession')
  }
  const interruptCall = interruptedPayloads[0] as { endpoint: string }
  if (interruptCall.endpoint !== '/api/v1/session-control/interrupt') {
    throw new Error('中断 click 应传入正确 endpoint')
  }
}

verifyBindSessionDetailControl().catch((error: unknown) => {
  throw error
})
