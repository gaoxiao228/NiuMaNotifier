import { bindSessionDetailControl, renderSessionDetailControl } from '../src/sessionWorkbenchView'
import type { ToolSessionDetail } from '../src/api'

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
