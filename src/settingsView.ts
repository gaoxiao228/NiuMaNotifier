import type { PluginManagementItem } from './api'
import { translations, type LanguageCode } from './i18n'
import { escapeHtml } from './viewUtils'

export type SettingsShellRenderOptions = {
  language: LanguageCode
}

export type PluginManagementRenderOptions = {
  element: HTMLElement | null
  language: LanguageCode
  plugins: PluginManagementItem[]
  busyPluginId: string | null
  importBusy: boolean
  resultText: string
}

export function renderSettingsShell(options: SettingsShellRenderOptions) {
  const t = translations[options.language]
  return `
    <aside class="settings-sidebar">
      <button class="settings-nav-item active" type="button" data-settings-panel="plugins">${escapeHtml(t.pluginManagement)}</button>
    </aside>
    <section class="settings-content">
      <div class="settings-heading">
        <div>
          <h2 id="settings-panel-title">${escapeHtml(t.pluginManagement)}</h2>
          <p>${escapeHtml(t.pluginManagementDescription)}</p>
        </div>
        <button id="plugin-import" type="button">${escapeHtml(t.importPlugin)}</button>
      </div>
      <div id="plugin-import-result" class="settings-result"></div>
      <div id="plugin-management-list" class="plugin-management-list"></div>
    </section>
  `
}

export function renderPluginManagement(options: PluginManagementRenderOptions) {
  if (!options.element) {
    return
  }
  const t = translations[options.language]
  if (options.plugins.length === 0) {
    options.element.innerHTML = `<p class="empty">${escapeHtml(t.noPlugins)}</p>`
    return
  }
  options.element.innerHTML = options.plugins
    .map((plugin) => {
      const busy = options.busyPluginId === plugin.id || isPluginTransitioning(plugin)
      return `
        <article class="plugin-card" data-plugin-id="${escapeHtml(plugin.id)}">
          <div class="plugin-card-main">
            <div>
              <h3>${escapeHtml(plugin.display_name)}</h3>
              <p>${escapeHtml(plugin.id)} · ${escapeHtml(plugin.tool_id)}</p>
            </div>
            <label class="plugin-enable-toggle">
              <span>${escapeHtml(plugin.enabled ? t.enabled : t.disabled)}</span>
              <input type="checkbox" data-plugin-toggle="${escapeHtml(plugin.id)}" ${plugin.enabled ? 'checked' : ''} ${busy ? 'disabled' : ''}>
            </label>
          </div>
          ${plugin.source === 'external' ? renderPluginActions(plugin, busy, t.removePlugin) : ''}
          <dl class="plugin-meta">
            <dt>${escapeHtml(t.pluginSource)}</dt>
            <dd>${escapeHtml(translatePluginSource(options.language, plugin.source))}</dd>
            <dt>${escapeHtml(t.pluginVersion)}</dt>
            <dd>${escapeHtml(plugin.version)}</dd>
            <dt>${escapeHtml(t.pluginRuntimeStatus)}</dt>
            <dd>${escapeHtml(translateRuntimeStatus(options.language, plugin.runtime_status))}</dd>
            <dt>${escapeHtml(t.pluginInstallPath)}</dt>
            <dd>${escapeHtml(plugin.install_path || t.none)}</dd>
            <dt>${escapeHtml(t.pluginLastError)}</dt>
            <dd>${escapeHtml(plugin.last_error || t.none)}</dd>
          </dl>
        </article>
      `
    })
    .join('')
}

function renderPluginActions(plugin: PluginManagementItem, busy: boolean, removeText: string) {
  return `
    <div class="plugin-card-actions">
      <button type="button" data-plugin-remove="${escapeHtml(plugin.id)}" ${busy ? 'disabled' : ''}>${escapeHtml(removeText)}</button>
    </div>
  `
}

export function renderPluginImportResult(
  element: HTMLElement | null,
  language: LanguageCode,
  text: string
) {
  if (!element) {
    return
  }
  const t = translations[language]
  element.textContent = text ? `${t.lastResult}: ${text}` : ''
}

export function translateRuntimeStatus(language: LanguageCode, status: string) {
  const t = translations[language]
  if (status === 'starting') {
    return t.pluginStarting
  }
  if (status === 'running') {
    return t.pluginRunning
  }
  if (status === 'stopping') {
    return t.pluginStopping
  }
  if (status === 'failed') {
    return t.pluginFailed
  }
  return t.pluginStopped
}

export function translatePluginSource(language: LanguageCode, source: string) {
  const t = translations[language]
  if (source === 'builtin') {
    return t.pluginBuiltin
  }
  return t.pluginExternal
}

function isPluginTransitioning(plugin: PluginManagementItem) {
  return plugin.runtime_status === 'starting' || plugin.runtime_status === 'stopping'
}
