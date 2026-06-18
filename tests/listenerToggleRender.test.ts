import { renderListenerToggle, renderListenerTools } from '../src/statusView'

class FakeTextElement {
  textContent = ''
  className = ''
}

class FakeInputElement {
  checked = false
  disabled = false
  attributes = new Map<string, string>()

  setAttribute(name: string, value: string) {
    this.attributes.set(name, value)
  }
}

function renderListener(enabled: boolean) {
  const toggle = new FakeInputElement()
  const label = new FakeTextElement()
  const state = new FakeTextElement()

  renderListenerToggle({
    toggle: toggle as unknown as HTMLInputElement,
    label: label as unknown as HTMLElement,
    state: state as unknown as HTMLElement,
    language: 'zh-CN',
    busy: false,
    enabled,
    loaded: true
  })

  return { toggle, label, state }
}

const disabledListener = renderListener(false)

if (disabledListener.label.className.includes('enabled')) {
  throw new Error('未监听时 Codex 监听标题不应使用绿色启用样式')
}

if (disabledListener.state.textContent !== '未监听') {
  throw new Error('未监听时应显示未监听状态文案')
}

const enabledListener = renderListener(true)

if (!enabledListener.label.className.includes('enabled')) {
  throw new Error('启用监听时 Codex 监听标题应使用绿色启用样式')
}

if (enabledListener.state.textContent !== '监听中') {
  throw new Error('启用监听时应显示监听中文案')
}

class FakeHtmlElement {
  innerHTML = ''
}

const list = new FakeHtmlElement()
renderListenerTools({
  element: list as unknown as HTMLElement,
  language: 'zh-CN',
  loaded: true,
  busyToolId: 'claude_code',
  tools: [
    {
      id: 'codex',
      plugin_id: 'builtin-codex',
      display_name: 'Codex',
      enabled: true,
      source: 'builtin',
      icon_url: null
    },
    {
      id: 'claude_code',
      plugin_id: 'claude-code',
      display_name: 'Claude Code',
      enabled: false,
      source: 'external',
      icon_url: null
    }
  ]
})

if (!list.innerHTML.includes('data-tool-toggle="codex"')) {
  throw new Error('工具监听列表应渲染 Codex 开关')
}

if (!list.innerHTML.includes('Claude Code')) {
  throw new Error('工具监听列表应渲染 Claude Code 名称')
}

if (!list.innerHTML.includes('disabled')) {
  throw new Error('保存中的工具开关应被禁用')
}
