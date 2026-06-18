import { renderPluginManagement, renderSettingsShell } from '../src/settingsView'

const shell = renderSettingsShell({ language: 'zh-CN' })

if (!shell.includes('插件管理')) {
  throw new Error('设置页左侧应包含插件管理入口')
}

if (!shell.includes('id="plugin-import"')) {
  throw new Error('插件管理页应渲染导入插件按钮')
}

class FakeElement {
  innerHTML = ''
}

const listElement = new FakeElement()

renderPluginManagement({
  element: listElement as HTMLElement,
  language: 'zh-CN',
  busyPluginId: null,
  importBusy: false,
  resultText: '',
  plugins: [
    {
      id: 'builtin-codex',
      tool_id: 'codex',
      display_name: 'Codex',
      version: '0.1.0',
      source: 'builtin',
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: null,
      install_path: null
    },
    {
      id: 'niuma-plugin-demo',
      tool_id: 'demo_tool',
      display_name: 'Demo Tool',
      version: '0.1.0',
      source: 'external',
      enabled: false,
      runtime_status: 'failed',
      last_error: '启动失败',
      icon_url: null,
      install_path: '/tmp/plugin'
    },
    {
      id: 'starting-plugin',
      tool_id: 'starting_tool',
      display_name: 'Starting Tool',
      version: '0.1.0',
      source: 'external',
      enabled: true,
      runtime_status: 'starting',
      last_error: null,
      icon_url: null,
      install_path: '/tmp/starting'
    },
    {
      id: 'stopping-plugin',
      tool_id: 'stopping_tool',
      display_name: 'Stopping Tool',
      version: '0.1.0',
      source: 'external',
      enabled: false,
      runtime_status: 'stopping',
      last_error: null,
      icon_url: null,
      install_path: '/tmp/stopping'
    }
  ]
})

if (!listElement.innerHTML.includes('data-plugin-toggle="builtin-codex"')) {
  throw new Error('插件列表应渲染内置插件开关')
}

if (listElement.innerHTML.includes('data-plugin-remove="builtin-codex"')) {
  throw new Error('内置插件不应渲染移除按钮')
}

if (!listElement.innerHTML.includes('Demo Tool')) {
  throw new Error('插件列表应渲染外部插件名称')
}

if (!listElement.innerHTML.includes('data-plugin-remove="niuma-plugin-demo"')) {
  throw new Error('外部插件应渲染移除按钮')
}

if (!listElement.innerHTML.includes('运行中') || !listElement.innerHTML.includes('失败')) {
  throw new Error('插件列表应渲染运行状态')
}

if (!listElement.innerHTML.includes('启动中') || !listElement.innerHTML.includes('停止中')) {
  throw new Error('插件列表应渲染过渡运行状态')
}

if (
  !listElement.innerHTML.includes('data-plugin-toggle="starting-plugin" checked disabled') ||
  !listElement.innerHTML.includes('data-plugin-remove="stopping-plugin" disabled')
) {
  throw new Error('过渡态插件应禁用开关和移除按钮')
}

if (!listElement.innerHTML.includes('启动失败')) {
  throw new Error('插件列表应渲染最近错误')
}
