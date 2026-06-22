import { renderDashboardShell } from '../src/dashboardLayout'

const html = renderDashboardShell()

if (html.includes('class="app-window"') || html.includes('class="window-titlebar"')) {
  throw new Error('主界面运行在真实 Tauri 窗口内，不应再渲染内嵌假窗口')
}

if (html.includes('id="status-tab"') || html.includes('id="notification-tab"')) {
  throw new Error('主界面不应继续渲染状态/通知页面切换')
}

if (html.includes('id="language-select"') || html.includes('id="refresh"')) {
  throw new Error('主界面不应显示语言选择和刷新按钮')
}

if (
  html.includes('id="session-list"') ||
  html.includes('id="session-overview"') ||
  html.includes('id="events"')
) {
  throw new Error('主界面不应继续渲染 Session 列表、Session 概览或最近事件')
}

if (!html.includes('id="main-status-panel"')) {
  throw new Error('主界面应渲染合并后的主状态面板')
}

if (!html.includes('id="settings-open"')) {
  throw new Error('主界面应渲染设置入口按钮')
}

if (!html.includes('class="settings-gear-icon"') || !html.includes('viewBox="0 0 24 24"')) {
  throw new Error('设置入口应渲染圆形齿轮 SVG 图标')
}

if (!html.includes('id="settings-view"')) {
  throw new Error('主界面应包含设置页容器')
}

if (!html.includes('id="tool-listener-list"')) {
  throw new Error('监听状态应渲染工具插件列表容器')
}

if (
  html.includes('id="codex-listener-detail"') ||
  html.includes('id="codex-listener-detail-label"') ||
  html.includes('id="codex-listener-detail-state"')
) {
  throw new Error('Codex 监听不应渲染重复的底部状态详情行')
}

if (!html.includes('id="local-sse-card" class="side-card" hidden')) {
  throw new Error('本地 SSE 接口应暂时隐藏')
}

if (!html.includes('id="notification-settings-card"')) {
  throw new Error('通知设置应作为右侧独立面板存在')
}

if (html.includes('id="notification-test"')) {
  throw new Error('主界面通知面板标题旁不应继续渲染全局测试通知按钮')
}

if (html.includes('id="notification-manage"')) {
  throw new Error('主界面通知插件不应显示管理按钮')
}

if (
  html.includes('id="notification-settings-details"') ||
  html.includes('id="notification-health"')
) {
  throw new Error('主界面不应继续渲染底部通知设置折叠区或通知健康摘要')
}

if (html.includes('status-legend')) {
  throw new Error('主界面只显示当前状态，不应渲染其他状态图例')
}

if (html.includes('class="status-quick-row"') || html.includes('id="status-quick-running"')) {
  throw new Error('主界面状态只能显示当前一个状态，不应渲染其他状态快捷条')
}
