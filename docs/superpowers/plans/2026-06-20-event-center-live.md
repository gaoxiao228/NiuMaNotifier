# Realtime Event Center Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a settings-sidebar Event Center that displays only realtime `NiumaEvent` messages received after the panel is opened.

**Architecture:** Keep the feature frontend-only. Add a focused renderer module for event-center HTML, extend the existing settings shell with an `event-center` panel, and manage the event SSE lifecycle in `src/main.ts` only while that panel is active. The backend SSE protocol stays unchanged.

**Tech Stack:** TypeScript, browser `EventSource`, Tauri frontend, existing translation helpers, existing hand-written TypeScript tests compiled by `tsc`.

---

## File Structure

- Create `src/eventCenterView.ts`: render empty/loading/connected/error states and event rows with collapsible JSON details.
- Create `tests/eventCenterRender.test.ts`: unit-style renderer checks for empty state, append order, escaped JSON, and expanded/collapsed behavior.
- Modify `src/settingsView.ts`: add `event-center` to `SettingsPanel`, side nav, and panel container.
- Modify `src/main.ts`: add event-center state, panel switching lifecycle, SSE connection, parsing, de-duplication, and expand/collapse handling.
- Modify `src/i18n.ts`: add event-center labels for all supported languages.
- Modify `src/styles.css`: style the event-center panel, rows, status pill, and JSON block.
- Modify `tests/settingsViewRender.test.ts`: assert event-center shell behavior.
- Modify `tests/responsiveLayoutCss.test.ts`: assert event-center responsive/scroll CSS.
- Modify `package.json`: add `test:event-center` and include it in `npm test`.

## Task 1: Event Center Renderer

**Files:**
- Create: `src/eventCenterView.ts`
- Create: `tests/eventCenterRender.test.ts`
- Modify: `package.json`

- [ ] **Step 1: Write the failing renderer test**

Create `tests/eventCenterRender.test.ts`:

```ts
import type { NiumaEvent } from '../src/api'
import { renderEventCenter } from '../src/eventCenterView'

class FakeElement {
  innerHTML = ''
}

const eventA = {
  id: 'event-a',
  tool: 'codex',
  session_id: 'session-a',
  project_name: 'NiuMaNotifier',
  project_path: '/repo/NiuMaNotifier',
  event_type: 'approval_requested',
  severity: 'urgent',
  summary: 'Bash: npm test',
  content: 'Run npm test',
  error_message: null,
  created_at: '2026-06-20T10:00:00Z'
} satisfies NiumaEvent

const eventB = {
  id: 'event-b',
  tool: 'codex',
  session_id: 'session-b',
  project_name: 'NiuMaNotifier',
  project_path: '/repo/NiuMaNotifier',
  event_type: 'task_failed',
  severity: 'urgent',
  summary: '<script>alert(1)</script>',
  content: null,
  error_message: 'assertion failed',
  created_at: '2026-06-20T10:01:00Z'
} satisfies NiumaEvent

const element = new FakeElement()

renderEventCenter({
  element: element as HTMLElement,
  language: 'zh-CN',
  events: [],
  expandedEventIds: new Set(),
  connected: false,
  connecting: true,
  errorText: ''
})

if (!element.innerHTML.includes('等待新的实时事件')) {
  throw new Error('事件中心空态应提示等待实时事件')
}

if (!element.innerHTML.includes('实时连接中')) {
  throw new Error('事件中心应展示连接中状态')
}

renderEventCenter({
  element: element as HTMLElement,
  language: 'zh-CN',
  events: [eventA, eventB],
  expandedEventIds: new Set(['event-b']),
  connected: true,
  connecting: false,
  errorText: ''
})

const firstIndex = element.innerHTML.indexOf('Bash: npm test')
const secondIndex = element.innerHTML.indexOf('&lt;script&gt;alert(1)&lt;/script&gt;')

if (firstIndex < 0 || secondIndex < 0 || firstIndex > secondIndex) {
  throw new Error('事件中心应按追加顺序渲染，新的实时事件出现在底部')
}

if (!element.innerHTML.includes('data-event-center-toggle="event-b"')) {
  throw new Error('事件项应提供点击展开目标')
}

if (!element.innerHTML.includes('class="event-center-json"')) {
  throw new Error('展开事件时应显示格式化 JSON 详情')
}

if (!element.innerHTML.includes('&quot;id&quot;: &quot;event-b&quot;')) {
  throw new Error('格式化 JSON 应经过 HTML 转义，避免原始事件内容注入页面')
}

if (!element.innerHTML.includes('实时已连接')) {
  throw new Error('事件中心应展示实时已连接状态')
}

renderEventCenter({
  element: element as HTMLElement,
  language: 'zh-CN',
  events: [eventA],
  expandedEventIds: new Set(),
  connected: false,
  connecting: false,
  errorText: '连接失败'
})

if (!element.innerHTML.includes('实时已断开') || !element.innerHTML.includes('连接失败')) {
  throw new Error('事件中心断开时应显示断开状态和错误文案')
}
```

- [ ] **Step 2: Add the test script and verify it fails**

Modify `package.json` scripts:

```json
{
  "test": "npm run test:main-dashboard-layout && npm run test:responsive-layout-css && npm run test:status-summary && npm run test:listener-toggle && npm run test:settings-view && npm run test:plugin-transition && npm run test:notification-settings-layout && npm run test:notification-history && npm run test:event-center",
  "test:event-center": "tsc --target ES2022 --module commonjs --moduleResolution node --lib ES2022,DOM --skipLibCheck --strict --esModuleInterop --outDir /tmp/niuma-event-center-test tests/eventCenterRender.test.ts && node /tmp/niuma-event-center-test/tests/eventCenterRender.test.js"
}
```

Run:

```bash
npm run test:event-center
```

Expected: fail with `Cannot find module '../src/eventCenterView'`.

- [ ] **Step 3: Implement the renderer**

Create `src/eventCenterView.ts`:

```ts
import type { NiumaEvent } from './api'
import {
  translateEventType,
  translateTool,
  translations,
  type LanguageCode
} from './i18n'
import { escapeHtml, formatLocalTime } from './viewUtils'

export type EventCenterRenderOptions = {
  element: HTMLElement | null
  language: LanguageCode
  events: NiumaEvent[]
  expandedEventIds: Set<string>
  connected: boolean
  connecting: boolean
  errorText: string
}

export function renderEventCenter(options: EventCenterRenderOptions) {
  if (!options.element) {
    return
  }
  const t = translations[options.language]
  const statusText = options.connected
    ? t.eventCenterConnected
    : options.connecting
      ? t.eventCenterConnecting
      : t.eventCenterDisconnected
  const statusClass = options.connected ? 'connected' : options.connecting ? 'connecting' : 'disconnected'
  options.element.innerHTML = `
    <div class="event-center-status-row">
      <span class="event-center-status ${statusClass}">${escapeHtml(statusText)}</span>
      ${options.errorText ? `<span class="event-center-error">${escapeHtml(options.errorText)}</span>` : ''}
    </div>
    <ol class="event-center-list">
      ${renderEventCenterItems(options)}
    </ol>
  `
}

function renderEventCenterItems(options: EventCenterRenderOptions) {
  const t = translations[options.language]
  if (options.events.length === 0) {
    return `<li class="empty">${escapeHtml(t.eventCenterWaiting)}</li>`
  }
  return options.events.map((event) => renderEventCenterItem(event, options)).join('')
}

function renderEventCenterItem(event: NiumaEvent, options: EventCenterRenderOptions) {
  const expanded = options.expandedEventIds.has(event.id)
  const detail = expanded
    ? `<pre class="event-center-json">${escapeHtml(JSON.stringify(event, null, 2))}</pre>`
    : ''
  // 每条事件只把摘要放在折叠行，完整原始字段统一交给 JSON 详情区展示。
  return `
    <li class="event-center-item ${expanded ? 'expanded' : ''}">
      <button class="event-center-row" type="button" data-event-center-toggle="${escapeHtml(event.id)}" aria-expanded="${expanded}">
        <strong>${escapeHtml(translateEventType(options.language, event.event_type))}</strong>
        <span>${escapeHtml(translateTool(options.language, event.tool))}</span>
        <span>${escapeHtml(event.project_name || translations[options.language].none)}</span>
        <span class="event-center-summary">${escapeHtml(event.summary || translations[options.language].none)}</span>
        <time>${escapeHtml(formatLocalTime(event.created_at, options.language))}</time>
      </button>
      ${detail}
    </li>
  `
}
```

- [ ] **Step 4: Run the renderer test**

Run:

```bash
npm run test:event-center
```

Expected: PASS.

- [ ] **Step 5: Commit renderer task**

```bash
git add package.json tests/eventCenterRender.test.ts src/eventCenterView.ts
git commit -m "feat: 新增实时事件中心渲染器" -m "修改内容：新增事件中心渲染模块和渲染测试，支持空态、连接状态、实时事件顺序和 JSON 展开详情。" -m "修改原因：为设置页事件中心提供独立 UI 渲染边界，避免把原始事件展示逻辑混入主流程。"
```

## Task 2: Settings Shell And Translations

**Files:**
- Modify: `src/settingsView.ts`
- Modify: `src/i18n.ts`
- Modify: `tests/settingsViewRender.test.ts`

- [ ] **Step 1: Extend the settings shell test**

Add these checks near the existing settings shell assertions in `tests/settingsViewRender.test.ts`:

```ts
if (!shell.includes('事件中心')) {
  throw new Error('设置页左侧应包含事件中心入口')
}

if (!shell.includes('data-settings-panel="event-center"')) {
  throw new Error('事件中心入口应声明设置页切换目标')
}

if (!shell.includes('id="settings-event-center"')) {
  throw new Error('设置页应渲染事件中心内容容器')
}

if (!shell.includes('id="settings-panel-event-center" class="settings-panel settings-event-center" hidden')) {
  throw new Error('插件管理默认页不应显示事件中心面板')
}
```

Add this active-panel check after the notification-history active check:

```ts
const eventCenterShell = renderSettingsShell({
  language: 'zh-CN',
  activePanel: 'event-center'
})

if (!eventCenterShell.includes('data-settings-panel="event-center" aria-current="page"')) {
  throw new Error('事件中心面板选中时应标记当前导航项')
}

if (
  !eventCenterShell.includes('id="settings-panel-plugins" class="settings-panel plugin-management-panel" hidden') ||
  !eventCenterShell.includes('id="settings-panel-notification-history" class="settings-panel settings-notification-history" hidden') ||
  eventCenterShell.includes('id="settings-panel-event-center" class="settings-panel settings-event-center" hidden')
) {
  throw new Error('事件中心应只在事件中心侧边栏面板中显示')
}
```

- [ ] **Step 2: Run the settings-view test and verify it fails**

Run:

```bash
npm run test:settings-view
```

Expected: fail with `设置页左侧应包含事件中心入口`.

- [ ] **Step 3: Add translation keys**

In `src/i18n.ts`, add these fields to the `Translation` type after `notificationHistory: string`:

```ts
  eventCenter: string
  eventCenterDescription: string
  eventCenterWaiting: string
  eventCenterConnected: string
  eventCenterConnecting: string
  eventCenterDisconnected: string
```

Add values to each locale object:

```ts
// zh-CN
eventCenter: '事件中心',
eventCenterDescription: '只显示打开面板后收到的实时 NiumaEvent。',
eventCenterWaiting: '等待新的实时事件',
eventCenterConnected: '实时已连接',
eventCenterConnecting: '实时连接中',
eventCenterDisconnected: '实时已断开',

// zh-TW
eventCenter: '事件中心',
eventCenterDescription: '只顯示開啟面板後收到的即時 NiumaEvent。',
eventCenterWaiting: '等待新的即時事件',
eventCenterConnected: '即時已連線',
eventCenterConnecting: '即時連線中',
eventCenterDisconnected: '即時已斷線',

// en
eventCenter: 'Event center',
eventCenterDescription: 'Shows only realtime NiumaEvent messages received after this panel opens.',
eventCenterWaiting: 'Waiting for realtime events',
eventCenterConnected: 'Realtime connected',
eventCenterConnecting: 'Realtime connecting',
eventCenterDisconnected: 'Realtime disconnected',

// ja
eventCenter: 'イベントセンター',
eventCenterDescription: 'このパネルを開いた後に受信したリアルタイム NiumaEvent のみを表示します。',
eventCenterWaiting: '新しいリアルタイムイベントを待機中',
eventCenterConnected: 'リアルタイム接続済み',
eventCenterConnecting: 'リアルタイム接続中',
eventCenterDisconnected: 'リアルタイム切断',

// ko
eventCenter: '이벤트 센터',
eventCenterDescription: '이 패널을 연 뒤 받은 실시간 NiumaEvent만 표시합니다.',
eventCenterWaiting: '새 실시간 이벤트 대기 중',
eventCenterConnected: '실시간 연결됨',
eventCenterConnecting: '실시간 연결 중',
eventCenterDisconnected: '실시간 연결 끊김',

// de
eventCenter: 'Ereigniszentrum',
eventCenterDescription: 'Zeigt nur Echtzeit-NiumaEvent-Meldungen, die nach dem Öffnen dieses Bereichs eingehen.',
eventCenterWaiting: 'Warten auf Echtzeitereignisse',
eventCenterConnected: 'Echtzeit verbunden',
eventCenterConnecting: 'Echtzeit verbindet',
eventCenterDisconnected: 'Echtzeit getrennt',
```

- [ ] **Step 4: Extend `renderSettingsShell`**

In `src/settingsView.ts`, change:

```ts
export type SettingsPanel = 'plugins' | 'notification-history'
```

to:

```ts
export type SettingsPanel = 'plugins' | 'event-center' | 'notification-history'
```

Inside `renderSettingsShell`, add:

```ts
  const eventCenterActive = activePanel === 'event-center'
```

Insert the nav button between plugins and notification history:

```ts
        <button class="settings-nav-item ${eventCenterActive ? 'active' : ''}" type="button" data-settings-panel="event-center" ${
          eventCenterActive ? 'aria-current="page"' : ''
        }>${escapeHtml(t.eventCenter)}</button>
```

Insert the panel between plugin management and notification history:

```ts
        <div id="settings-panel-event-center" class="settings-panel settings-event-center" ${
          eventCenterActive ? '' : 'hidden'
        }>
          <div class="settings-heading">
            <div>
              <h2>${escapeHtml(t.eventCenter)}</h2>
              <p>${escapeHtml(t.eventCenterDescription)}</p>
            </div>
          </div>
          <div id="settings-event-center" class="event-center-shell"></div>
        </div>
```

- [ ] **Step 5: Run settings tests**

Run:

```bash
npm run test:settings-view
npm run test:event-center
```

Expected: both PASS.

- [ ] **Step 6: Commit shell task**

```bash
git add src/settingsView.ts src/i18n.ts tests/settingsViewRender.test.ts
git commit -m "feat: 在设置页新增事件中心入口" -m "修改内容：扩展设置页侧边栏和面板结构，新增事件中心多语言文案和对应渲染测试。" -m "修改原因：为实时事件中心提供独立设置页入口，并保持插件管理、事件中心和通知历史的面板边界清晰。"
```

## Task 3: Realtime SSE Lifecycle

**Files:**
- Modify: `src/main.ts`
- Test: `npm run build`

- [ ] **Step 1: Add event-center imports and state**

In `src/main.ts`, add `NiumaEvent` to the API type imports:

```ts
  type MainStatePayload,
  type NiumaEvent,
  type NotificationRecord,
```

Add the renderer import:

```ts
import { renderEventCenter } from './eventCenterView'
```

Add the path constant after `stateStreamPath`:

```ts
const eventStreamPath = '/api/v1/events/stream'
```

Add state after notification history state:

```ts
let eventCenterEvents: NiumaEvent[] = []
let eventCenterExpandedIds = new Set<string>()
let eventCenterStream: EventSource | undefined
let eventCenterStreamConnected = false
let eventCenterStreamConnecting = false
let eventCenterErrorText = ''
```

- [ ] **Step 2: Add render and lifecycle helpers**

Add these functions near `renderSettingsNotificationHistory()`:

```ts
function renderSettingsEventCenter() {
  renderEventCenter({
    element: document.querySelector<HTMLElement>('#settings-event-center'),
    language: currentLanguage,
    events: eventCenterEvents,
    expandedEventIds: eventCenterExpandedIds,
    connected: eventCenterStreamConnected,
    connecting: eventCenterStreamConnecting,
    errorText: eventCenterErrorText
  })
}

function startEventCenterStream() {
  if (eventCenterStream || activeView !== 'settings' || activeSettingsPanel !== 'event-center') {
    return
  }
  eventCenterEvents = []
  eventCenterExpandedIds = new Set()
  eventCenterStreamConnected = false
  eventCenterStreamConnecting = true
  eventCenterErrorText = ''
  renderSettingsEventCenter()
  void connectEventCenterStream()
}

async function connectEventCenterStream() {
  try {
    const apiUrl = await getLocalApiUrl()
    if (activeView !== 'settings' || activeSettingsPanel !== 'event-center') {
      return
    }
    eventCenterStream = new EventSource(`${apiUrl}${eventStreamPath}`)
    eventCenterStream.onopen = () => {
      eventCenterStreamConnected = true
      eventCenterStreamConnecting = false
      eventCenterErrorText = ''
      renderSettingsEventCenter()
    }
    eventCenterStream.addEventListener('event', (message) => {
      appendEventCenterEvent((message as MessageEvent<string>).data)
    })
    eventCenterStream.onerror = () => {
      eventCenterStreamConnected = false
      eventCenterStreamConnecting = false
      eventCenterErrorText = translations[currentLanguage].eventCenterDisconnected
      renderSettingsEventCenter()
    }
  } catch (error) {
    eventCenterStreamConnected = false
    eventCenterStreamConnecting = false
    eventCenterErrorText = error instanceof Error ? error.message : String(error)
    renderSettingsEventCenter()
  }
}

function stopEventCenterStream() {
  eventCenterStream?.close()
  eventCenterStream = undefined
  eventCenterStreamConnected = false
  eventCenterStreamConnecting = false
  eventCenterErrorText = ''
}

function appendEventCenterEvent(data: string) {
  try {
    const nextEvent = JSON.parse(data) as NiumaEvent
    if (eventCenterEvents.some((event) => event.id === nextEvent.id)) {
      return
    }
    // 事件中心是实时观察窗口，新消息按到达顺序追加到底部。
    eventCenterEvents = [...eventCenterEvents, nextEvent]
    renderSettingsEventCenter()
  } catch (error) {
    eventCenterErrorText = error instanceof Error ? error.message : String(error)
    renderSettingsEventCenter()
  }
}
```

- [ ] **Step 3: Wire rendering and panel switching**

In `renderSettings()`, after `renderSettingsNotificationHistory()` add:

```ts
  renderSettingsEventCenter()
```

In `showDashboardView()`, before `renderActiveView()` add:

```ts
  stopEventCenterStream()
```

In `showSettingsView()`, after the notification-history lazy load block add:

```ts
  if (activeSettingsPanel === 'event-center') {
    startEventCenterStream()
  }
```

In `applyLanguage()`, after `renderSettings()` keep `renderSettings()` as the single shell refresh, then call:

```ts
  if (activeSettingsPanel === 'event-center') {
    renderSettingsEventCenter()
  }
```

In the settings click handler, replace:

```ts
  if (settingsPanel === 'plugins' || settingsPanel === 'notification-history') {
    activeSettingsPanel = settingsPanel
    renderSettings()
    if (activeSettingsPanel === 'notification-history' && !notificationRecordsLoaded) {
      await refreshNotificationRecords()
    }
    return
  }
```

with:

```ts
  if (
    settingsPanel === 'plugins' ||
    settingsPanel === 'event-center' ||
    settingsPanel === 'notification-history'
  ) {
    stopEventCenterStream()
    activeSettingsPanel = settingsPanel
    renderSettings()
    if (activeSettingsPanel === 'notification-history' && !notificationRecordsLoaded) {
      await refreshNotificationRecords()
    }
    if (activeSettingsPanel === 'event-center') {
      startEventCenterStream()
    }
    return
  }
```

Add this click branch before plugin config handling:

```ts
  const eventCenterToggleId = target?.dataset.eventCenterToggle
  if (eventCenterToggleId) {
    if (eventCenterExpandedIds.has(eventCenterToggleId)) {
      eventCenterExpandedIds.delete(eventCenterToggleId)
    } else {
      eventCenterExpandedIds.add(eventCenterToggleId)
    }
    renderSettingsEventCenter()
    return
  }
```

- [ ] **Step 4: Run TypeScript build**

Run:

```bash
npm run build
```

Expected: PASS. If TypeScript reports duplicate or missing translation fields, fix `src/i18n.ts` before continuing.

- [ ] **Step 5: Commit SSE lifecycle task**

```bash
git add src/main.ts
git commit -m "feat: 接入事件中心实时事件流" -m "修改内容：在设置页事件中心激活时连接 events SSE，收到 NiumaEvent 后去重并追加到底部，离开面板时关闭连接。" -m "修改原因：让事件中心符合只显示打开面板后实时事件的需求，并避免后台长期占用事件流。"
```

## Task 4: Event Center Layout Styles

**Files:**
- Modify: `src/styles.css`
- Modify: `tests/responsiveLayoutCss.test.ts`

- [ ] **Step 1: Add failing CSS assertions**

Append to `tests/responsiveLayoutCss.test.ts` before the media-query assertions:

```ts
if (
  !css.includes('.settings-event-center') ||
  !css.includes('grid-template-rows: auto minmax(0, 1fr);') ||
  !css.includes('height: 100%;')
) {
  throw new Error('事件中心面板应填满右侧区域，并让实时事件列表占据标题下方剩余空间')
}

if (
  !css.includes('.event-center-list') ||
  !css.includes('overflow: auto;') ||
  !css.includes('min-height: 0;')
) {
  throw new Error('事件中心列表应在面板剩余区域内独立滚动')
}

if (
  !css.includes('.event-center-json') ||
  !css.includes('max-height: 220px;') ||
  !css.includes('overflow: auto;')
) {
  throw new Error('事件中心 JSON 详情应限制高度并在块内滚动')
}
```

- [ ] **Step 2: Run CSS test and verify it fails**

Run:

```bash
npm run test:responsive-layout-css
```

Expected: fail with `事件中心面板应填满右侧区域`.

- [ ] **Step 3: Add CSS**

In `src/styles.css`, place this near the existing settings panel and notification-history styles:

```css
.settings-event-center {
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  gap: 12px;
  height: 100%;
}

.event-center-shell {
  min-height: 0;
  display: grid;
  grid-template-rows: auto minmax(0, 1fr);
  gap: 10px;
}

.event-center-status-row {
  display: flex;
  align-items: center;
  gap: 10px;
  min-width: 0;
}

.event-center-status {
  border: 1px solid rgba(148, 163, 184, 0.65);
  border-radius: 999px;
  padding: 4px 10px;
  font-size: 12px;
  line-height: 1.3;
  color: #475569;
  background: #f8fafc;
  white-space: nowrap;
}

.event-center-status.connected {
  border-color: rgba(34, 197, 94, 0.45);
  color: #166534;
  background: #f0fdf4;
}

.event-center-status.connecting {
  border-color: rgba(245, 158, 11, 0.45);
  color: #92400e;
  background: #fffbeb;
}

.event-center-status.disconnected {
  border-color: rgba(239, 68, 68, 0.45);
  color: #991b1b;
  background: #fef2f2;
}

.event-center-error {
  min-width: 0;
  color: #991b1b;
  font-size: 12px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.event-center-list {
  min-height: 0;
  overflow: auto;
  display: flex;
  flex-direction: column;
  gap: 8px;
  margin: 0;
  padding: 0;
  list-style: none;
}

.event-center-list > li.empty {
  color: #64748b;
  padding: 18px 4px;
}

.event-center-item {
  border: 1px solid rgba(148, 163, 184, 0.35);
  border-radius: 8px;
  background: #fff;
  overflow: hidden;
}

.event-center-item.expanded {
  border-color: rgba(71, 85, 105, 0.45);
}

.event-center-row {
  width: 100%;
  display: grid;
  grid-template-columns: minmax(104px, 0.9fr) minmax(72px, 0.65fr) minmax(110px, 1fr) minmax(140px, 2fr) minmax(128px, auto);
  gap: 10px;
  align-items: center;
  border: 0;
  padding: 10px 12px;
  color: #334155;
  background: transparent;
  text-align: left;
  cursor: pointer;
}

.event-center-row:hover {
  background: #f8fafc;
}

.event-center-row > * {
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.event-center-row strong {
  color: #0f172a;
}

.event-center-summary {
  color: #475569;
}

.event-center-row time {
  color: #64748b;
  text-align: right;
}

.event-center-json {
  max-height: 220px;
  overflow: auto;
  margin: 0;
  padding: 12px;
  border-top: 1px solid rgba(148, 163, 184, 0.28);
  color: #dbeafe;
  background: #0f172a;
  font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, "Liberation Mono", monospace;
  font-size: 12px;
  line-height: 1.55;
}

@media (max-width: 720px) {
  .event-center-row {
    grid-template-columns: minmax(0, 1fr);
  }

  .event-center-row time {
    text-align: left;
  }
}
```

- [ ] **Step 4: Run CSS and renderer tests**

Run:

```bash
npm run test:responsive-layout-css
npm run test:event-center
```

Expected: both PASS.

- [ ] **Step 5: Commit CSS task**

```bash
git add src/styles.css tests/responsiveLayoutCss.test.ts
git commit -m "feat: 完善事件中心布局样式" -m "修改内容：新增事件中心面板、实时状态、事件行和 JSON 详情块样式，并补充响应式 CSS 测试。" -m "修改原因：保证实时事件列表在设置页内独立滚动，避免长 JSON 或窄屏导致布局溢出。"
```

## Task 5: Final Verification

**Files:**
- Verify only unless failures require fixes.

- [ ] **Step 1: Run all frontend tests**

Run:

```bash
npm test
```

Expected: PASS, including `test:event-center`.

- [ ] **Step 2: Run production frontend build**

Run:

```bash
npm run build
```

Expected: PASS.

- [ ] **Step 3: Check git diff**

Run:

```bash
git status --short
git diff --check
```

Expected: `git diff --check` exits 0. `git status --short` shows only files intentionally modified by this feature.

- [ ] **Step 4: Commit any verification fixes**

If Steps 1-3 required corrections, commit them:

```bash
git add src tests package.json
git commit -m "fix: 修正事件中心验证问题" -m "修改内容：修正测试或构建发现的事件中心前端问题。" -m "修改原因：保证实时事件中心通过完整前端测试和生产构建。"
```

If no corrections were needed, do not create an empty commit.

## Self-Review

- Spec coverage: the plan adds a settings sidebar entry, shows only realtime `NiumaEvent` messages, uses `/api/v1/events/stream`, appends messages at the bottom, supports collapsed rows plus JSON expansion, closes SSE when leaving the panel, and does not add history loading or backend replay.
- Placeholder scan: checked for forbidden placeholder patterns; none remain.
- Type consistency: `SettingsPanel` uses `'event-center'`; renderer accepts `NiumaEvent`; SSE path is `eventStreamPath`; expansion state uses `Set<string>` consistently.
