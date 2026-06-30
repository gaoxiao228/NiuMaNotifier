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
    last_error: null,
    active_connection_id: 'conn_1',
    selected_transport: 'webrtc',
    available_transports: ['relay', 'webrtc']
  },
  busyAction: null,
  resultText: '',
  diagnosticReport: {
    scope: 'local_agent',
    overall: 'passed',
    summary: 'remoteDiagnosticsSummaryPassed',
    started_at: '2026-06-30T00:00:00.000Z',
    finished_at: '2026-06-30T00:00:01.000Z',
    steps: [
      {
        key: 'binding',
        title: 'remoteDiagnosticsStepBinding',
        status: 'passed'
      }
    ]
  }
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

if (!html.includes('当前远程连接') || !html.includes('conn_1')) {
  throw new Error('远程访问设置应显示当前活动连接')
}

if (html.includes('id="remote-login"')) {
  throw new Error('已登录并绑定后不应继续显示登录并绑定按钮')
}

if (!html.includes('可用通道') || !html.includes('Relay') || !html.includes('WebRTC')) {
  throw new Error('连接状态应显示当前可用的远程通道')
}

if (!html.includes('正在使用') || !html.includes('WebRTC')) {
  throw new Error('连接状态应显示当前正在使用的通道')
}

if (!html.includes('一键诊断') || !html.includes('远程访问诊断')) {
  throw new Error('远程访问设置应显示一键诊断入口和诊断报告')
}

const savedResponseHtml = renderRemoteSettingsPanel({
  language: 'zh-CN',
  settings: {
    server_url: 'https://remote.example.com',
    remote_access_enabled: true,
    remote_control_enabled: true,
    user: { id: 'user_1', email: 'user@example.com', role: 'owner' },
    device: { id: 'dev_1', name: 'NiuMa MacBook' },
    bound: false,
    has_credential: false,
    last_connected_at: null
  },
  agentStatus: null,
  busyAction: null,
  resultText: ''
})

if (!savedResponseHtml.includes('已绑定')) {
  throw new Error('保存设置后的摘要只要仍有账号和设备信息，就不应显示未绑定')
}

const onlineWithoutClientHtml = renderRemoteSettingsPanel({
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
    last_error: null,
    active_connection_id: null,
    selected_transport: null,
    available_transports: []
  },
  busyAction: null,
  resultText: ''
})

if (!onlineWithoutClientHtml.includes('远程状态') || !onlineWithoutClientHtml.includes('在线')) {
  throw new Error('设备信令连接已建立时，远程状态应显示在线')
}

if (!onlineWithoutClientHtml.includes('当前远程连接') || !onlineWithoutClientHtml.includes('无外部客户端连接')) {
  throw new Error('没有外部客户端连接时，应明确显示当前无外部客户端连接')
}

if (onlineWithoutClientHtml.includes('可用通道') || onlineWithoutClientHtml.includes('正在使用')) {
  throw new Error('没有外部客户端连接时，不应显示可用通道和正在使用通道')
}
