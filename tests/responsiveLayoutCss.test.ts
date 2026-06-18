declare function require(name: string): { readFileSync(path: string, encoding: string): string }

const { readFileSync } = require('fs')
const css = readFileSync('src/styles.css', 'utf8')

if (!css.includes('grid-template-columns: minmax(0, 1fr) minmax(320px, 430px);')) {
  throw new Error('桌面主布局应保持弹性两列，避免中等窗口被挤成竖向布局')
}

if (!css.includes('@media (max-width: 720px)')) {
  throw new Error('主界面只应在移动端宽度切换为竖向布局')
}

if (css.includes('@media (max-width: 980px)')) {
  throw new Error('980px 断点会导致桌面窗口过早变成竖向布局')
}
