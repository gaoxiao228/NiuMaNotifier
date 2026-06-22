import { translations, type LanguageCode } from './i18n'
import { escapeHtml } from './viewUtils'

export function renderEventCenterWindowShell(language: LanguageCode) {
  const t = translations[language]
  // 独立窗口只提供事件中心容器，实时连接和列表由 eventCenterWindow.ts 管理。
  return `
    <section class="event-center-window">
      <header class="event-center-window-header">
        <h1>${escapeHtml(t.eventCenter)}</h1>
      </header>
      <div id="event-center-root" class="event-center-shell"></div>
    </section>
  `
}
