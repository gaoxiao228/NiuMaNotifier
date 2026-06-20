declare function require(name: string): { readFileSync(path: string, encoding: string): string }

const { readFileSync } = require('fs')
const css = readFileSync('src/styles.css', 'utf8')

// 只截取目标 selector 的第一段声明，避免全文件同名属性导致误判。
function cssRule(selector: string) {
  const escaped = selector.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
  const match = css.match(new RegExp(`${escaped}\\s*\\{([^}]*)\\}`))
  return match?.[1] ?? ''
}

function ruleIncludes(selector: string, property: string) {
  return cssRule(selector).includes(property)
}

function cssBlockAfter(marker: string) {
  const start = css.indexOf(marker)
  if (start < 0) {
    return ''
  }
  const nextMedia = css.indexOf('@media', start + marker.length)
  return css.slice(start, nextMedia < 0 ? undefined : nextMedia)
}

// 校验 media 块内的同一条规则，避免 selector 和属性分别来自不同规则。
function mediaRuleIncludes(mediaMarker: string, selectorPart: string, property: string) {
  const block = cssBlockAfter(mediaMarker)
  const rules = block.match(/[^{}]+\{[^}]*\}/g) ?? []
  return rules.some((rule) => {
    const [selector, body = ''] = rule.split('{')
    return selector.includes(selectorPart) && body.includes(property)
  })
}

if (!css.includes('grid-template-columns: minmax(0, 1fr) minmax(320px, 430px);')) {
  throw new Error('桌面主布局应保持弹性两列，避免中等窗口被挤成竖向布局')
}

if (!css.includes('.dashboard-grid') || !css.includes('align-items: start;')) {
  throw new Error('桌面主布局应顶部对齐，让主状态和右侧栏目按内容自适应高度')
}

if (css.includes('min-height: calc(100vh - 146px);')) {
  throw new Error('主状态面板不应使用视口高度撑满，应按当前状态内容自适应')
}

if (css.includes('grid-template-rows: minmax(0, auto) 1fr;')) {
  throw new Error('右侧栏目不应把通知区域拉伸到剩余高度，应按监听和通知内容自适应')
}

if (css.includes('.listener-card {\n  min-height: 346px;')) {
  throw new Error('监听状态卡片不应固定大高度，应按工具监听项数量自适应')
}

if (css.includes('.icon-action::before') || css.includes('.icon-action::after')) {
  throw new Error('设置按钮图标应使用明确的圆形齿轮 SVG，不应再用伪元素拼接')
}

if (
  !css.includes('.settings-view') ||
  !css.includes('height: 100vh;') ||
  !css.includes('grid-template-rows: auto minmax(0, 1fr);')
) {
  throw new Error('设置页应占满视口，并让主体区域吃掉返回按钮之外的剩余高度')
}

if (
  !css.includes('.settings-layout') ||
  !css.includes('height: 100%;') ||
  !css.includes('align-items: stretch;')
) {
  throw new Error('设置页布局应使用固定父容器高度，并让左右区域填满父视图高度')
}

if (css.includes('height: calc(100vh - 174px);')) {
  throw new Error('设置页不应使用固定像素扣减视口高度，避免底部留白过大')
}

if (
  !css.includes('.settings-sidebar') ||
  !css.includes('align-self: stretch;') ||
  !css.includes('height: 100%;')
) {
  throw new Error('设置页左侧侧边栏高度应填满父视图，而不是按内容高度收缩')
}

if (
  !css.includes('.settings-content') ||
  !css.includes('height: 100%;') ||
  !css.includes('overflow: hidden;')
) {
  throw new Error('设置页右侧内容区应固定高度，滚动交给具体面板内部区域')
}

if (
  !css.includes('.plugin-management-panel') ||
  !css.includes('grid-template-rows: auto minmax(0, 1fr);') ||
  !css.includes('height: 100%;')
) {
  throw new Error('插件管理面板应固定标题，并让下方区域占据剩余高度')
}

if (
  !css.includes('.plugin-management-scroll') ||
  !css.includes('min-height: 0;') ||
  !css.includes('overflow: auto;')
) {
  throw new Error('插件管理面板下方区域应独立滚动')
}

if (css.includes('max-height: 460px;') || css.includes('.plugin-management-list {\n  display: grid;\n  gap: 10px;\n  max-height')) {
  throw new Error('插件列表本身不应再有内部滚动高度限制，避免 scrollview 套 scrollview')
}

if (
  !css.includes('.settings-notification-history') ||
  !css.includes('grid-template-rows: auto minmax(0, 1fr);') ||
  !css.includes('height: 100%;')
) {
  throw new Error('通知历史面板应填满右侧区域，并让历史列表占据标题下方剩余空间')
}

if (css.includes('max-height: calc(100vh - 260px);')) {
  throw new Error('通知历史列表不应使用固定视口扣减高度，避免底部异常空白')
}

if (
  !css.includes('.notification-history-list') ||
  !css.includes('min-height: 0;') ||
  !css.includes('overflow: auto;')
) {
  throw new Error('通知历史列表应在面板剩余区域内滚动')
}

if (css.includes('display: contents;')) {
  throw new Error('通知历史条目不应使用 display: contents 展开内部字段，避免正文和元信息重叠')
}

if (
  !css.includes('.notification-record-card') ||
  !css.includes('.notification-record-header') ||
  !css.includes('.notification-record-meta') ||
  !css.includes('.notification-record-detail')
) {
  throw new Error('通知历史条目应有分层卡片布局样式')
}

// 事件中心应继承设置页固定高度布局，内部列表和 JSON 详情各自滚动。
if (
  !ruleIncludes('.settings-event-center', 'display: grid;') ||
  !ruleIncludes('.settings-event-center', 'grid-template-rows: auto minmax(0, 1fr);') ||
  !ruleIncludes('.settings-event-center', 'height: 100%;')
) {
  throw new Error('事件中心面板应填满右侧区域，并让实时事件列表占据标题下方剩余空间')
}

if (
  !ruleIncludes('.event-center-shell', 'display: grid;') ||
  !ruleIncludes('.event-center-shell', 'grid-template-rows: auto minmax(0, 1fr);') ||
  !ruleIncludes('.event-center-shell', 'min-height: 0;')
) {
  throw new Error('事件中心内容壳应延续固定高度网格链路，让列表获得可滚动剩余空间')
}

if (
  !ruleIncludes('.event-center-list', 'overflow: auto;') ||
  !ruleIncludes('.event-center-list', 'min-height: 0;')
) {
  throw new Error('事件中心列表应在面板剩余区域内独立滚动')
}

if (!ruleIncludes('.event-center-list > li', 'flex: 0 0 auto;')) {
  throw new Error('事件中心列表项不应在 flex 列表中被压缩，应保持自身高度后交给列表滚动')
}

if (
  !ruleIncludes('.event-center-row', 'display: grid;') ||
  !ruleIncludes(
    '.event-center-row',
    'grid-template-columns: minmax(96px, 0.8fr) minmax(64px, 0.55fr) minmax(110px, 0.9fr) minmax(180px, 1.6fr) minmax(136px, auto);'
  )
) {
  throw new Error('事件中心事件行应在桌面端保持多列网格布局')
}

if (
  !ruleIncludes('.event-center-row', 'font-size: 13px;') ||
  !ruleIncludes('.event-center-row', 'font-weight: 500;') ||
  !ruleIncludes('.event-center-row', 'line-height: 1.35;') ||
  !ruleIncludes('.event-center-row', 'min-height: 42px;')
) {
  throw new Error('事件中心事件行应使用紧凑字号和稳定行高，避免实时事件列表显得拥挤')
}

if (
  !ruleIncludes('.event-center-row > *', 'min-width: 0;') ||
  !ruleIncludes('.event-center-row > *', 'overflow: hidden;') ||
  !ruleIncludes('.event-center-row > *', 'text-overflow: ellipsis;') ||
  !ruleIncludes('.event-center-row > *', 'white-space: nowrap;')
) {
  throw new Error('事件中心事件行子项应允许收缩并截断长文本')
}

if (
  !ruleIncludes('.event-center-json', 'max-height: 160px;') ||
  !ruleIncludes('.event-center-json', 'overflow: auto;')
) {
  throw new Error('事件中心 JSON 详情应限制高度并在块内滚动')
}

if (
  !ruleIncludes('.event-center-detail', 'overflow: hidden;') ||
  !ruleIncludes('.event-center-detail', 'animation: event-center-detail-expand 180ms ease-out;')
) {
  throw new Error('事件中心详情应在当前事件下方使用展开动画，而不是通过滚动跳转展示')
}

if (
  !css.includes('@keyframes event-center-detail-expand') ||
  !css.includes('@media (prefers-reduced-motion: reduce)') ||
  !css.includes('.event-center-detail {\n    animation: none;')
) {
  throw new Error('事件中心详情展开动画应有关键帧，并尊重减少动态效果设置')
}

if (!css.includes('@media (max-width: 720px)')) {
  throw new Error('主界面只应在移动端宽度切换为竖向布局')
}

if (!mediaRuleIncludes('@media (max-width: 720px)', '.event-center-row', 'grid-template-columns: 1fr;')) {
  throw new Error('事件中心事件行应在 720px 移动端断点栈叠为单列布局')
}

if (css.includes('@media (max-width: 980px)')) {
  throw new Error('980px 断点会导致桌面窗口过早变成竖向布局')
}
