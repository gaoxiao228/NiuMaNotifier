import { renderEventCenterWindowShell } from '../src/eventCenterWindowView'

const html = renderEventCenterWindowShell('zh-CN')

if (!html.includes('class="event-center-window"')) {
  throw new Error('事件中心窗口应使用独立窗口外壳')
}

if (!html.includes('<h1>事件中心</h1>')) {
  throw new Error('事件中心窗口应展示本地化标题')
}

if (!html.includes('id="event-center-root"')) {
  throw new Error('事件中心窗口应提供实时列表挂载容器')
}

const englishHtml = renderEventCenterWindowShell('en')

if (!englishHtml.includes('<h1>Event center</h1>')) {
  throw new Error('事件中心窗口标题应支持英文')
}
