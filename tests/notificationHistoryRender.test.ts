import { renderNotificationHistoryOnly } from '../src/notificationView'
import type { NotificationRecord } from '../src/api'

class FakeElement {
  innerHTML = ''
}

const formElement = new FakeElement()
const historyElement = new FakeElement()

formElement.innerHTML = '<input data-field="device_key" value="draft-key">'

renderNotificationHistoryOnly({
  historyElement: historyElement as HTMLOListElement,
  language: 'zh-CN',
  recordsLoaded: true,
  records: [
    {
      id: 'record-1',
      event_id: 'event-1',
      event_type: 'assistant_message_completed',
      channel: 'bark',
      status: 'sent',
      title: '测试通知',
      body: '通知内容',
      reason: 'completed',
      error_message: null,
      created_at: '2026-06-16T00:00:00Z',
      sent_at: '2026-06-16T00:00:01Z'
    } satisfies NotificationRecord,
    {
      id: 'record-2',
      event_id: 'event-2',
      event_type: 'approval_requested',
      channel: 'builtin-bark',
      plugin_id: 'builtin-bark',
      status: 'failed',
      title: 'Bark 插件通知',
      body: null,
      reason: 'approval_requested',
      error_message: 'Device Key 未配置',
      created_at: '2026-06-16T01:00:00Z',
      sent_at: null
    } satisfies NotificationRecord
  ]
})

if (!historyElement.innerHTML.includes('测试通知')) {
  throw new Error('通知历史刷新没有渲染最新记录')
}

if (
  !historyElement.innerHTML.includes('class="notification-record-card"') ||
  !historyElement.innerHTML.includes('class="notification-record-header"') ||
  !historyElement.innerHTML.includes('class="notification-record-meta"') ||
  !historyElement.innerHTML.includes('class="notification-record-detail"')
) {
  throw new Error('通知历史条目应按头部、元信息和详情分层渲染，避免内容重叠')
}

if (
  !historyElement.innerHTML.includes('builtin-bark') ||
  !historyElement.innerHTML.includes('Device Key 未配置')
) {
  throw new Error('通知历史应渲染插件渠道和失败原因')
}

if (
  !historyElement.innerHTML.includes('原因：') ||
  !historyElement.innerHTML.includes('创建时间：') ||
  !historyElement.innerHTML.includes('发送时间：')
) {
  throw new Error('通知历史元信息标签应带中文冒号')
}

if (formElement.innerHTML !== '<input data-field="device_key" value="draft-key">') {
  throw new Error('通知历史刷新不应修改通知配置表单')
}
