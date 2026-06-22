import { renderPluginManagement, renderSettingsShell } from '../src/settingsView'

const shell = renderSettingsShell({ language: 'zh-CN' })

if (!shell.includes('class="settings-layout"')) {
  throw new Error('设置页应使用设计稿中的侧边导航加内容区布局')
}

if (!shell.includes('插件管理')) {
  throw new Error('设置页左侧应包含插件管理入口')
}

if (!shell.includes('通知历史')) {
  throw new Error('设置页左侧应包含通知历史入口')
}

if (shell.includes('data-settings-panel="event-center"')) {
  throw new Error('设置页不应再包含事件中心入口')
}

if (!shell.includes('data-settings-panel="notification-history"')) {
  throw new Error('通知历史入口应声明设置页切换目标')
}

if (shell.includes('id="settings-event-center"')) {
  throw new Error('设置页不应再渲染事件中心内容容器')
}

if (!shell.includes('id="settings-notification-history"')) {
  throw new Error('设置页应渲染通知历史列表容器')
}

if (!shell.includes('id="settings-panel-plugins" class="settings-panel plugin-management-panel"')) {
  throw new Error('插件管理默认页应显示插件管理面板')
}

if (
  !shell.includes('class="plugin-management-scroll"') ||
  !shell.includes('id="plugin-import-result"') ||
  !shell.includes('id="plugin-management-list"')
) {
  throw new Error('插件管理页应固定标题，并将结果和插件列表放在下方滚动区域')
}

if (!shell.includes('id="settings-panel-notification-history" class="settings-panel settings-notification-history" hidden')) {
  throw new Error('插件管理默认页不应在下方同时展示通知历史区域')
}

if (shell.includes('id="settings-panel-event-center"')) {
  throw new Error('设置页不应再渲染事件中心面板')
}

if (!shell.includes('data-settings-panel="plugins" aria-current="page"')) {
  throw new Error('设置页默认应选中插件管理')
}

const historyShell = renderSettingsShell({
  language: 'zh-CN',
  activePanel: 'notification-history'
})

if (!historyShell.includes('data-settings-panel="notification-history" aria-current="page"')) {
  throw new Error('通知历史面板选中时应标记当前导航项')
}

if (
  !historyShell.includes('id="settings-panel-plugins" class="settings-panel plugin-management-panel" hidden') ||
  historyShell.includes('id="settings-panel-notification-history" class="settings-panel settings-notification-history" hidden')
) {
  throw new Error('通知历史应只在通知历史侧边栏面板中显示')
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
      capabilities: ['event_watcher'],
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: '/assets/codex-icon.png',
      config_schema: [],
      install_path: null,
      management_actions: [
        {
          id: 'codex_hook_install',
          label: '安装 Hook',
          description: '接收 Codex 权限请求并回传允许/拒绝结果',
          kind: 'primary',
          enabled: true,
          status_label: '未安装',
          status_level: 'neutral'
        }
      ]
    },
    {
      id: 'builtin-bark',
      kind: 'notification',
      tool_id: null,
      display_name: 'Bark',
      version: '0.1.0',
      source: 'builtin',
      capabilities: ['event_consumer', 'notification_test'],
      enabled: false,
      runtime_status: 'stopped',
      last_error: null,
      icon_url: '/assets/bark-icon.png',
      config_schema: [
        {
          key: 'device_key',
          type: 'string',
          label: 'Device Key',
          required: true
        }
      ],
      install_path: null
    },
    {
      id: 'builtin-ntfy',
      kind: 'notification',
      tool_id: null,
      display_name: 'ntfy',
      version: '0.1.0',
      source: 'builtin',
      capabilities: ['event_consumer', 'notification_test'],
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: '/assets/ntfy-logo.svg',
      config_schema: [
        {
          key: 'topic',
          type: 'string',
          label: 'Topic',
          required: true
        }
      ],
      install_path: null
    },
    {
      id: 'approval-menu',
      kind: 'notification',
      tool_id: null,
      display_name: 'Approval Menu',
      version: '0.1.0',
      source: 'external',
      capabilities: ['event_consumer', 'approval_handler'],
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: null,
      config_schema: [],
      install_path: '/tmp/approval-menu'
    },
    {
      id: 'niuma-plugin-demo',
      tool_id: 'demo_tool',
      display_name: 'Demo Tool',
      version: '0.1.0',
      source: 'external',
      capabilities: ['event_watcher'],
      enabled: false,
      runtime_status: 'failed',
      last_error: '启动失败',
      icon_url: null,
      config_schema: [],
      install_path: '/tmp/plugin'
    },
    {
      id: 'starting-plugin',
      tool_id: 'starting_tool',
      display_name: 'Starting Tool',
      version: '0.1.0',
      source: 'external',
      capabilities: ['event_watcher'],
      enabled: true,
      runtime_status: 'starting',
      last_error: null,
      icon_url: null,
      config_schema: [],
      install_path: '/tmp/starting'
    },
    {
      id: 'stopping-plugin',
      tool_id: 'stopping_tool',
      display_name: 'Stopping Tool',
      version: '0.1.0',
      source: 'external',
      capabilities: ['event_watcher'],
      enabled: false,
      runtime_status: 'stopping',
      last_error: null,
      icon_url: null,
      config_schema: [],
      install_path: '/tmp/stopping'
    },
    {
      id: 'status-indicator-demo',
      kind: 'status_indicator',
      tool_id: null,
      display_name: '状态指示 Demo',
      version: '0.1.0',
      source: 'external',
      capabilities: ['state_consumer'],
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: null,
      config_schema: [
        {
          key: 'style',
          type: 'select',
          label: '显示样式',
          required: false,
          options: ['indicator', 'pet']
        }
      ],
      install_path: '/tmp/status-indicator'
    }
  ],
  busyConfigPluginId: null,
  configResultText: '',
  pluginConfigs: {
    'builtin-bark': {
      device_key: 'device-1'
    },
    'builtin-ntfy': {
      topic: 'niuma-topic'
    },
    'status-indicator-demo': {
      style: 'pet'
    }
  }
})

if (!listElement.innerHTML.includes('data-plugin-toggle="builtin-codex"')) {
  throw new Error('插件列表应渲染内置插件开关')
}

if (
  !listElement.innerHTML.includes('class="plugin-management-actions"') ||
  !listElement.innerHTML.includes('data-plugin-action-plugin="builtin-codex"') ||
  !listElement.innerHTML.includes('data-plugin-action-id="codex_hook_install"') ||
  !listElement.innerHTML.includes('安装 Hook') ||
  !listElement.innerHTML.includes('未安装')
) {
  throw new Error('插件管理页应按 management_actions 通用渲染插件管理动作')
}

if (listElement.innerHTML.includes('data-plugin-action-id="codex_hook_uninstall"')) {
  throw new Error('同一 Hook 状态下插件管理页不应同时渲染安装和移除动作')
}

const codexCardStart = listElement.innerHTML.indexOf('data-plugin-id="builtin-codex"')
const nextCardStart = listElement.innerHTML.indexOf('data-plugin-id="builtin-bark"')
const codexCardHtml = listElement.innerHTML.slice(codexCardStart, nextCardStart)
const codexSideStart = codexCardHtml.indexOf('class="plugin-card-side"')

if (codexSideStart === -1) {
  throw new Error('插件管理动作应渲染在插件卡片右侧操作区')
}

if (!codexCardHtml.slice(codexSideStart).includes('class="plugin-management-actions"')) {
  throw new Error('插件管理动作应放在右侧操作区内')
}

if (codexCardHtml.slice(0, codexSideStart).includes('class="plugin-management-actions"')) {
  throw new Error('插件管理动作不应继续挤在左侧插件信息区')
}

if (
  !listElement.innerHTML.includes('data-plugin-toggle="builtin-bark"') ||
  !listElement.innerHTML.includes('data-plugin-toggle="builtin-ntfy"') ||
  listElement.innerHTML.includes('data-plugin-toggle="builtin-bark"  disabled') ||
  listElement.innerHTML.includes('data-plugin-toggle="builtin-ntfy"  disabled')
) {
  throw new Error('notification 插件应渲染可用开关')
}

if (
  !listElement.innerHTML.includes('data-plugin-notification-test="builtin-bark"') ||
  !listElement.innerHTML.includes('data-plugin-notification-test="builtin-ntfy"')
) {
  throw new Error('插件管理页应为支持 notification_test 的通知插件渲染测试通知按钮')
}

if (listElement.innerHTML.includes('data-plugin-notification-test="approval-menu"')) {
  throw new Error('不支持 notification_test 的通知插件不应在插件管理页渲染测试通知按钮')
}

if (
  !listElement.innerHTML.includes('class="plugin-icon image"') ||
  !listElement.innerHTML.includes('src="/assets/codex-icon.png"') ||
  !listElement.innerHTML.includes('alt="Codex"') ||
  !listElement.innerHTML.includes('src="/assets/bark-icon.png"') ||
  !listElement.innerHTML.includes('alt="Bark"') ||
  !listElement.innerHTML.includes('src="/assets/ntfy-logo.svg"') ||
  !listElement.innerHTML.includes('alt="ntfy"')
) {
  throw new Error('插件管理页应渲染 manifest 提供的插件图标')
}

if (
  !listElement.innerHTML.includes('class="plugin-icon fallback"') ||
  !listElement.innerHTML.includes('aria-label="Demo Tool"') ||
  !listElement.innerHTML.includes('>D</span>')
) {
  throw new Error('插件管理页应为缺少 icon_url 的插件渲染稳定 fallback 图标')
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

const demoCardStart = listElement.innerHTML.indexOf('data-plugin-id="niuma-plugin-demo"')
const demoInfoIndex = listElement.innerHTML.indexOf('class="plugin-card-info"', demoCardStart)
const demoMetaIndex = listElement.innerHTML.indexOf('class="plugin-meta"', demoCardStart)
const demoRemoveIndex = listElement.innerHTML.indexOf('data-plugin-remove="niuma-plugin-demo"', demoCardStart)
const demoBodyEnd = listElement.innerHTML.indexOf('</div>', demoRemoveIndex)
if (
  demoCardStart < 0 ||
  demoInfoIndex < 0 ||
  demoMetaIndex < 0 ||
  demoRemoveIndex < 0 ||
  demoInfoIndex > demoMetaIndex ||
  demoMetaIndex > demoRemoveIndex ||
  demoRemoveIndex > demoBodyEnd
) {
  throw new Error('移除插件按钮应放在左侧插件信息区域底部')
}

if (!listElement.innerHTML.includes('运行中') || !listElement.innerHTML.includes('失败')) {
  throw new Error('插件列表应渲染运行状态')
}

if (
  !listElement.innerHTML.includes('Approval Menu') ||
  !listElement.innerHTML.includes('approval-menu · 通知插件') ||
  !listElement.innerHTML.includes('插件能力') ||
  !listElement.innerHTML.includes('授权处理')
) {
  throw new Error('插件管理应展示 approval_handler 的授权处理能力标签')
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

if (
  !listElement.innerHTML.includes('data-plugin-toggle="status-indicator-demo" checked') ||
  !listElement.innerHTML.includes('status-indicator-demo · 状态指示插件') ||
  !listElement.innerHTML.includes('data-plugin-config-save="status-indicator-demo"') ||
  !listElement.innerHTML.includes('<option value="pet" selected>pet</option>')
) {
  throw new Error('状态指示插件应在插件管理中按独立插件类型渲染并支持配置')
}

if (
  !listElement.innerHTML.includes('data-plugin-config-save="builtin-bark"') ||
  !listElement.innerHTML.includes('id="plugin-config-builtin-bark-device_key" type="text"') ||
  listElement.innerHTML.includes('id="plugin-config-builtin-bark-device_key" type="password"') ||
  !listElement.innerHTML.includes('value="device-1"')
) {
  throw new Error('带配置 schema 的插件应渲染插件配置表单')
}

const barkCardStart = listElement.innerHTML.indexOf('data-plugin-id="builtin-bark"')
const barkMetaIndex = listElement.innerHTML.indexOf('class="plugin-meta"', barkCardStart)
const barkConfigIndex = listElement.innerHTML.indexOf('class="plugin-config-form"', barkCardStart)
if (barkCardStart < 0 || barkMetaIndex < 0 || barkConfigIndex < 0 || barkMetaIndex > barkConfigIndex) {
  throw new Error('插件管理卡片应先渲染插件信息，再渲染插件配置，保证信息在左侧、配置在右侧')
}

if (
  !listElement.innerHTML.includes('data-plugin-config-save="builtin-ntfy"') ||
  !listElement.innerHTML.includes('id="plugin-config-builtin-ntfy-topic" type="text"') ||
  !listElement.innerHTML.includes('value="niuma-topic"')
) {
  throw new Error('ntfy 插件应在插件管理中渲染 Topic 配置表单')
}

const installedHookElement = new FakeElement()

renderPluginManagement({
  element: installedHookElement as HTMLElement,
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
      capabilities: ['event_watcher'],
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: '/assets/codex-icon.png',
      config_schema: [],
      install_path: null,
      management_actions: [
        {
          id: 'codex_hook_uninstall',
          label: '移除 Hook',
          description:
            'Niuma Hook 已写入 Codex 配置。仍需在 Codex 中执行 /hooks 并信任 Niuma Hook，信任后才能接收权限请求。',
          kind: 'danger',
          enabled: true,
          status_label: 'Hook 已安装，需在 /hooks 中信任',
          status_level: 'warning'
        }
      ]
    }
  ],
  busyConfigPluginId: null,
  configResultText: '',
  pluginConfigs: {}
})

if (
  !installedHookElement.innerHTML.includes('data-plugin-action-id="codex_hook_uninstall"') ||
  !installedHookElement.innerHTML.includes('Hook 已安装，需在 /hooks 中信任') ||
  !installedHookElement.innerHTML.includes('执行 /hooks 并信任 Niuma Hook') ||
  !installedHookElement.innerHTML.includes('plugin-management-action-status warning')
) {
  throw new Error('已安装 Hook 的管理动作应提示用户仍需通过 /hooks 信任')
}
