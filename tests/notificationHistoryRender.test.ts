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
    } satisfies NotificationRecord
  ]
})

if (!historyElement.innerHTML.includes('测试通知')) {
  throw new Error('通知历史刷新没有渲染最新记录')
}

if (formElement.innerHTML !== '<input data-field="device_key" value="draft-key">') {
  throw new Error('通知历史刷新不应修改通知配置表单')
}
