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
  channels: [
    {
      channel: 'bark',
      enabled: true,
      payload: { device_key: 'bark-device-key' }
    },
    {
      channel: 'ntfy',
      enabled: true,
      payload: { topic: 'codex-notifier' }
    }
  ],
  resultText: '',
  busyChannel: null
})

if (!formElement.innerHTML.includes('notification-field-row')) {
  throw new Error('通知设置字段应使用标签和输入框同排布局')
}

if (!formElement.innerHTML.includes('notification-compact-title')) {
  throw new Error('通知设置标题应使用紧凑样式')
}

if (!formElement.innerHTML.includes('data-channel="bark"')) {
  throw new Error('通知设置应渲染 Bark 配置块')
}

if (!formElement.innerHTML.includes('data-channel="ntfy"')) {
  throw new Error('通知设置应渲染 ntfy 配置块')
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

const mixedResult = formatNotificationTestResult('zh-CN', ['bark'], [
  { channel: 'ntfy', message: 'topic 无效' }
])

if (!mixedResult.includes('已发送: bark') || !mixedResult.includes('错误: ntfy: topic 无效')) {
  throw new Error('测试通知结果应同时展示成功渠道和失败渠道')
}
