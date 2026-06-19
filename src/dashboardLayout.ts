export function renderDashboardShell() {
  // 主界面只保留当前工作状态、监听控制、本地 SSE 状态和通知设置，避免分散到多页。
  return `
    <section class="shell">
      <header class="topbar">
        <div class="brand">
          <h1>NiumaNotifier</h1>
          <p id="subtitle"></p>
        </div>
        <button id="settings-open" class="icon-action" type="button"></button>
      </header>
      <main id="dashboard-view" class="dashboard-grid">
        <section id="main-status-panel" class="status-panel">
          <div class="panel-heading">
            <h2 id="current-status-label"></h2>
            <span id="updated" class="updated" hidden>-</span>
          </div>
          <div id="status-summary" class="status-summary"></div>
          <button id="clear-blocker" class="secondary-action" type="button" hidden></button>
          <dl id="request-detail" class="request-detail" hidden></dl>
        </section>
        <aside class="side-panel">
          <section id="codex-listener-card" class="side-card">
            <h2 id="listener-health-title"></h2>
            <div id="tool-listener-list" class="listener-tool-list"></div>
            <p id="codex-listener-description" class="listener-description"></p>
          </section>
          <section id="local-sse-card" class="side-card" hidden>
            <h2 id="local-sse-title"></h2>
            <dl class="endpoint-summary">
              <dt id="local-sse-state-label"></dt>
              <dd id="local-sse-state"></dd>
              <dt id="local-sse-port-label"></dt>
              <dd id="local-sse-port"></dd>
              <dt id="local-sse-path-label"></dt>
              <dd id="local-sse-path"></dd>
              <dt id="local-sse-url-label"></dt>
              <dd id="local-sse-url"></dd>
            </dl>
          </section>
          <section id="notification-settings-card" class="side-card">
            <div class="notification-settings-heading">
              <h2 id="notification-settings-title"></h2>
              <div class="notification-actions">
                <!-- 通知插件在主界面只保留测试入口，配置管理统一放到设置页。 -->
                <button id="notification-test" type="button" data-action="test"></button>
              </div>
            </div>
            <div id="notification-form" class="notification-form"></div>
          </section>
        </aside>
      </main>
      <main id="settings-view" class="settings-view" hidden>
        <div class="settings-topline">
          <button id="settings-back" type="button"></button>
        </div>
        <div id="settings-shell" class="settings-shell"></div>
      </main>
    </section>
  `
}
