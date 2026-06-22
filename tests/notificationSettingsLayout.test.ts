import {
  formatNotificationTestResult,
  renderNotificationPage
} from '../src/notificationView'

class FakeElement {
  textContent = ''
  innerHTML = ''
}

const formElement = new FakeElement()
const titleElement = new FakeElement()

renderNotificationPage({
  formElement: formElement as HTMLElement,
  settingsTitleElement: titleElement as HTMLElement,
  language: 'zh-CN',
  notificationPlugins: [
    {
      id: 'builtin-bark',
      kind: 'notification',
      tool_id: null,
      display_name: 'Bark',
      version: '0.1.0',
      source: 'builtin',
      capabilities: ['event_consumer', 'notification_test'],
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: '/assets/bark-icon.png',
      config_schema: [],
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
      enabled: false,
      runtime_status: 'stopped',
      last_error: null,
      icon_url: null,
      config_schema: [],
      install_path: null
    },
    {
      id: 'webhook-plugin',
      kind: 'notification',
      tool_id: null,
      display_name: 'Webhook',
      version: '0.1.0',
      source: 'external',
      capabilities: ['event_consumer', 'notification_test'],
      enabled: true,
      runtime_status: 'running',
      last_error: null,
      icon_url: null,
      config_schema: [],
      install_path: '/tmp/webhook-plugin'
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
    }
  ],
  busyPluginId: null
})

if (titleElement.textContent !== '通知插件') {
  throw new Error('主界面通知面板标题应改为通知插件')
}

if (
  !formElement.innerHTML.includes('data-notification-plugin-id="builtin-bark"') ||
  !formElement.innerHTML.includes('data-notification-plugin-toggle="builtin-ntfy"') ||
  !formElement.innerHTML.includes('data-notification-plugin-id="webhook-plugin"')
) {
  throw new Error('主界面应根据 notification 类型插件渲染通知插件列表')
}

if (
  !formElement.innerHTML.includes('class="plugin-icon image"') ||
  !formElement.innerHTML.includes('src="/assets/bark-icon.png"') ||
  !formElement.innerHTML.includes('alt="Bark"')
) {
  throw new Error('主界面推送插件卡片应渲染 manifest 提供的插件图标')
}

if (
  !formElement.innerHTML.includes('class="plugin-icon fallback"') ||
  !formElement.innerHTML.includes('aria-label="Webhook"') ||
  !formElement.innerHTML.includes('>W</span>')
) {
  throw new Error('主界面推送插件卡片应为缺少 icon_url 的插件渲染稳定 fallback 图标')
}

if (formElement.innerHTML.includes('data-action="save"')) {
  throw new Error('通知设置表单不应再渲染保存按钮')
}

if (formElement.innerHTML.includes('data-notification-plugin-test')) {
  throw new Error('主界面通知插件摘要不应渲染测试通知按钮，测试入口应放在插件管理页')
}

if (formElement.innerHTML.includes('notification-result')) {
  throw new Error('首页通知插件区域不应显示最近结果')
}

const mixedResult = formatNotificationTestResult('zh-CN', ['builtin-ntfy'], [
  { pluginId: 'builtin-bark', message: 'device key 无效' }
])

if (
  !mixedResult.includes('已发送: builtin-ntfy') ||
  !mixedResult.includes('错误: builtin-bark: device key 无效')
) {
  throw new Error('测试通知结果应同时展示成功插件和失败插件')
}
