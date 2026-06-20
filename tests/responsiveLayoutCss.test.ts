declare function require(name: string): { readFileSync(path: string, encoding: string): string }

const { readFileSync } = require('fs')
const css = readFileSync('src/styles.css', 'utf8')

function cssBlock(selector: string) {
  const start = css.indexOf(`${selector} {`)
  if (start === -1) {
    return ''
  }
  const end = css.indexOf('\n}', start)
  return end === -1 ? css.slice(start) : css.slice(start, end + 2)
}

const shellBlock = cssBlock('.shell')
const dashboardGridBlock = cssBlock('.dashboard-grid')
const statusPanelBlock = cssBlock('.status-panel')
const statusCardBlock = cssBlock('.status-card')
const pluginIconImageBlock = cssBlock('.plugin-icon.image')
const pluginIconImgBlock = cssBlock('.plugin-icon img')
const pluginCardMainBlock = cssBlock('.plugin-card-main')

if (!css.includes('grid-template-columns: minmax(0, 1fr) minmax(320px, 430px);')) {
  throw new Error('桌面主布局应保持弹性两列，避免中等窗口被挤成竖向布局')
}

if (
  !shellBlock.includes('display: grid;') ||
  !shellBlock.includes('grid-template-rows: auto minmax(0, 1fr);')
) {
  throw new Error('主界面外壳应使用顶部栏加剩余视图两行布局，让内容区可填满父视图')
}

if (!dashboardGridBlock.includes('align-items: stretch;')) {
  throw new Error('桌面主布局应拉伸网格项，让主状态区域默认填充到父视图底部')
}

if (
  !statusPanelBlock.includes('display: grid;') ||
  !statusPanelBlock.includes('grid-template-rows: auto minmax(0, 1fr);')
) {
  throw new Error('主状态面板应让标题占自然高度，并让状态内容填满剩余高度')
}

if (!statusCardBlock.includes('background: #ffffff;') || !statusCardBlock.includes('height: 100%;')) {
  throw new Error('主状态卡片应填满主状态面板剩余高度')
}

if (
  !statusPanelBlock.includes('min-width: 0;') ||
  !statusCardBlock.includes('min-width: 0;') ||
  !css.includes('.request-detail dd {\n  color: #17213a;') ||
  !css.includes('margin: 0;\n  min-width: 0;\n  overflow-wrap: anywhere;')
) {
  throw new Error('主状态区域应允许长请求内容在左侧网格内收缩换行，避免覆盖右侧状态卡片')
}

if (
  css.includes('.status-card:has(.status-summary.info)') ||
  css.includes('.status-card:has(.status-summary.warning)')
) {
  throw new Error('主状态卡片不应按状态给整块区域填色，语义色只应保留在圆点和标签上')
}

if (
  !css.includes('.status-card {\n  background: #ffffff;') ||
  !css.includes('border: 1px solid #d5e0ee;')
) {
  throw new Error('主状态卡片应使用中性白底和边框，避免空闲状态被大面积绿色强调')
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
  pluginIconImageBlock.includes('background: #ffffff;') ||
  pluginIconImageBlock.includes('border: 1px solid') ||
  !pluginIconImgBlock.includes('height: 34px;') ||
  !pluginIconImgBlock.includes('width: 34px;') ||
  !pluginIconImgBlock.includes('object-fit: cover;')
) {
  throw new Error('真实插件图标应填满图标区域，不应出现白色内框或四周留白')
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
  !pluginCardMainBlock.includes('display: grid;') ||
  !pluginCardMainBlock.includes('grid-template-columns: auto minmax(0, 1fr) auto;') ||
  !pluginCardMainBlock.includes('align-items: start;') ||
  pluginCardMainBlock.includes('justify-content: space-between;')
) {
  throw new Error('插件卡片顶部应按图标、左对齐信息、右侧开关三列布局')
}

if (
  css.includes('grid-template-columns: minmax(0, 1fr) 292px;') ||
  !css.includes('grid-template-columns: minmax(260px, 0.85fr) minmax(320px, 1fr);') ||
  !css.includes('.plugin-card-info {\n  display: grid;') ||
  !css.includes('grid-column: 1;') ||
  !css.includes('.plugin-meta {\n  background: #f8fafc;') ||
  !css.includes('.plugin-config-form {\n  display: grid;') ||
  !css.includes('grid-column: 2;')
) {
  throw new Error('插件管理卡片应统一左侧显示插件信息，右侧显示插件配置')
}

if (
  !css.includes('.plugin-card-info .plugin-card-actions {\n  justify-content: flex-start;') ||
  !css.includes('margin-top: 10px;')
) {
  throw new Error('移除插件按钮应放在左侧插件信息区底部并左对齐')
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

if (
  css.includes('grid-template-columns: minmax(96px, 1.2fr) 110px') ||
  css.includes('grid-template-columns: minmax(140px, 1.1fr) max-content') ||
  !css.includes('grid-template-columns: max-content max-content minmax(180px, 1fr) max-content;')
) {
  throw new Error('通知历史第一行标题、状态和插件名称应按内容宽度连续左对齐')
}

if (!css.includes('justify-items: start;')) {
  throw new Error('通知历史第一行的状态标记和插件名称应在各自列内左对齐')
}

if (
  !css.includes('.notification-record-meta {\n  display: flex;') ||
  css.includes('grid-template-columns: 72px minmax(0, 1fr) 72px minmax(0, 1fr) 72px minmax(0, 1fr);') ||
  !css.includes('column-gap: 0;') ||
  !css.includes('margin: 0 32px 0 0;')
) {
  throw new Error('通知历史元信息标签和值应紧贴显示，只在字段组之间保留间距')
}

if (
  !css.includes('.notification-record-channel {\n  overflow-wrap: anywhere;') ||
  css.includes('.notification-record-title,\n.notification-record-channel {\n  color: #53627a;\n  font-size: 12px;\n  min-width: 0;\n  overflow: hidden;')
) {
  throw new Error('通知历史插件名称不应被单行省略，应允许换行完整显示')
}

if (!css.includes('@media (max-width: 720px)')) {
  throw new Error('主界面只应在移动端宽度切换为竖向布局')
}

if (css.includes('@media (max-width: 980px)')) {
  throw new Error('980px 断点会导致桌面窗口过早变成竖向布局')
}
