import { renderRemoteSettingsPanel, renderSettingsShell } from '../src/settingsView'

const shell = renderSettingsShell({ language: 'zh-CN', activePanel: 'remote-access' })

if (!shell.includes('data-settings-panel="remote-access" aria-current="page"')) {
  throw new Error('远程访问面板选中时应标记当前导航项')
}

if (!shell.includes('id="settings-panel-remote-access" class="settings-panel remote-settings-panel"')) {
  throw new Error('设置页应渲染远程访问面板容器')
}

const html = renderRemoteSettingsPanel({
  language: 'zh-CN',
  settings: {
    server_url: 'https://remote.example.com',
    remote_access_enabled: true,
    remote_control_enabled: true,
    user: { id: 'user_1', email: 'user@example.com', role: 'owner' },
    device: { id: 'dev_1', name: 'NiuMa MacBook' },
    bound: true,
    has_credential: true,
    last_connected_at: null
  },
  agentStatus: {
    state: 'online',
    last_error: null
  },
  busyAction: null,
  resultText: ''
})

if (!html.includes('value="https://remote.example.com"')) {
  throw new Error('远程设置应渲染服务端地址')
}

if (!html.includes('user@example.com') || !html.includes('NiuMa MacBook')) {
  throw new Error('已绑定状态应显示账号和设备摘要')
}

if (html.includes('device_token')) {
  throw new Error('远程设置页面不能渲染 device_token')
}

if (!html.includes('远程状态') || !html.includes('在线')) {
  throw new Error('远程访问设置应显示 RemoteAgent 状态')
}
