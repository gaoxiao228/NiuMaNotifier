import { renderListenerToggle } from '../src/statusView'

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
