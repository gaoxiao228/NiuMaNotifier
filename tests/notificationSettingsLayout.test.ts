import {
  formatNotificationTestResult,
  renderNotificationPage,
  renderNotificationResult
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
      icon_url: null,
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
    }
  ],
  resultText: '',
  busyPluginId: null
})

if (titleElement.textContent !== '通知插件') {
  throw new Error('主界面通知面板标题应改为通知插件')
}

if (
  !formElement.innerHTML.includes('data-notification-plugin-id="builtin-bark"') ||
  !formElement.innerHTML.includes('data-notification-plugin-toggle="builtin-ntfy"')
) {
  throw new Error('主界面应根据 notification 类型插件渲染通知插件列表')
}

if (formElement.innerHTML.includes('data-action="save"')) {
  throw new Error('通知设置表单不应再渲染保存按钮')
}

if (formElement.innerHTML.includes('data-action="test"')) {
  throw new Error('测试通知按钮应放到通知设置标题旁边，不应在表单底部')
}

class FakeResultElement {
  textContent = ''
}

class FakeFormWithResult {
  resultElement = new FakeResultElement()

  querySelector(selector: string) {
    return selector === '.notification-result' ? this.resultElement : null
  }
}

const formWithResult = new FakeFormWithResult()

renderNotificationResult(formWithResult as unknown as HTMLElement, 'zh-CN', '已自动保存')

if (formWithResult.resultElement.textContent !== '最近结果: 已自动保存') {
  throw new Error('自动保存应只更新最近结果文本')
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
