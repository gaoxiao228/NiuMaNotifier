import type { PluginConfigField, PluginManagementItem } from './api'
import { translations, type LanguageCode } from './i18n'
import { renderPluginIcon } from './pluginIcon'
import { escapeHtml } from './viewUtils'

export type SettingsShellRenderOptions = {
  language: LanguageCode
  activePanel?: SettingsPanel
}

export type SettingsPanel = 'plugins' | 'notification-history'

export type PluginManagementRenderOptions = {
  element: HTMLElement | null
  language: LanguageCode
  plugins: PluginManagementItem[]
  busyPluginId: string | null
  busyActionKey?: string | null
  busyConfigPluginId: string | null
  importBusy: boolean
  resultText: string
  actionResultText?: string
  configResultText: string
  notificationTestBusyPluginId?: string | null
  notificationTestResultPluginId?: string | null
  notificationTestResultText?: string
  pluginConfigs: Record<string, Record<string, unknown>>
}

export function renderSettingsShell(options: SettingsShellRenderOptions) {
  const t = translations[options.language]
  const activePanel = options.activePanel ?? 'plugins'
  const pluginsActive = activePanel === 'plugins'
  const notificationHistoryActive = activePanel === 'notification-history'
  return `
    <div class="settings-layout">
      <aside class="settings-sidebar">
        <button class="settings-nav-item ${pluginsActive ? 'active' : ''}" type="button" data-settings-panel="plugins" ${
          pluginsActive ? 'aria-current="page"' : ''
        }>${escapeHtml(t.pluginManagement)}</button>
        <button class="settings-nav-item ${notificationHistoryActive ? 'active' : ''}" type="button" data-settings-panel="notification-history" ${
          notificationHistoryActive ? 'aria-current="page"' : ''
        }>${escapeHtml(t.notificationHistory)}</button>
      </aside>
      <section class="settings-content">
        <div id="settings-panel-plugins" class="settings-panel plugin-management-panel" ${pluginsActive ? '' : 'hidden'}>
          <div class="settings-heading">
            <div>
              <h2 id="settings-panel-title">${escapeHtml(t.pluginManagement)}</h2>
              <p>${escapeHtml(t.pluginManagementDescription)}</p>
            </div>
            <button id="plugin-import" type="button">${escapeHtml(t.importPlugin)}</button>
          </div>
          <div class="plugin-management-scroll">
            <div id="plugin-import-result" class="settings-result"></div>
            <div id="plugin-management-list" class="plugin-management-list"></div>
          </div>
        </div>
        <div id="settings-panel-notification-history" class="settings-panel settings-notification-history" ${
          notificationHistoryActive ? '' : 'hidden'
        }>
          <div class="settings-heading">
            <div>
              <h2>${escapeHtml(t.notificationHistory)}</h2>
            </div>
            <button id="settings-notification-history-refresh" type="button">${escapeHtml(t.refresh)}</button>
          </div>
          <ol id="settings-notification-history" class="notification-history-list"></ol>
        </div>
      </section>
    </div>
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
      const pluginType = translatePluginKind(options.language, plugin.kind ?? 'tool')
      const pluginSubtitle = plugin.tool_id
        ? `${plugin.id} · ${plugin.tool_id}`
        : `${plugin.id} · ${pluginType}`
      return `
        <article class="plugin-card" data-plugin-id="${escapeHtml(plugin.id)}">
          <div class="plugin-card-main">
            ${renderPluginIcon(plugin)}
            <div>
              <h3>${escapeHtml(plugin.display_name)} <span class="plugin-runtime-inline ${escapeHtml(
                plugin.runtime_status
              )}">${escapeHtml(translateRuntimeStatus(options.language, plugin.runtime_status))}</span></h3>
              <p>${escapeHtml(pluginSubtitle)}</p>
            </div>
            <label class="plugin-enable-toggle">
              <span>${escapeHtml(plugin.enabled ? t.enabled : t.disabled)}</span>
              <input type="checkbox" data-plugin-toggle="${escapeHtml(plugin.id)}" ${plugin.enabled ? 'checked' : ''} ${busy ? 'disabled' : ''}>
            </label>
          </div>
          <div class="plugin-card-body">
            <div class="plugin-card-info">
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
              <div class="plugin-capabilities" aria-label="${escapeHtml(t.pluginCapabilities)}">
                <span class="plugin-capabilities-label">${escapeHtml(t.pluginCapabilities)}</span>
                <span class="plugin-capability-list">
                  ${plugin.capabilities
                    .map(
                      (capability) =>
                        `<span class="plugin-capability">${escapeHtml(
                          translatePluginCapability(options.language, capability)
                        )}</span>`
                    )
                    .join('')}
                </span>
              </div>
              ${plugin.source === 'external' ? renderPluginActions(plugin, busy, t.removePlugin) : ''}
            </div>
            <div class="plugin-card-side">
              ${renderPluginManagementActions(
                plugin,
                options.busyActionKey ?? null,
                options.actionResultText ?? ''
              )}
              ${renderPluginConfigForm(
                plugin,
                options.pluginConfigs[plugin.id] ?? {},
                options.busyConfigPluginId === plugin.id,
                options.configResultText,
                t
              )}
              ${renderPluginNotificationTestAction(
                plugin,
                options.notificationTestBusyPluginId === plugin.id,
                options.notificationTestResultPluginId === plugin.id
                  ? options.notificationTestResultText ?? ''
                  : '',
                t
              )}
            </div>
          </div>
        </article>
      `
    })
    .join('')
}

function renderPluginManagementActions(
  plugin: PluginManagementItem,
  busyActionKey: string | null,
  actionResultText: string
) {
  const actions = plugin.management_actions ?? []
  if (actions.length === 0) {
    return ''
  }
  return `
    <div class="plugin-management-actions">
      ${actions
        .map((action) => {
          const actionKey = pluginActionKey(plugin.id, action.id)
          const busy = busyActionKey === actionKey
          return `
            <div class="plugin-management-action">
              <div>
                <p class="plugin-management-action-title">${escapeHtml(action.label)}</p>
                <p class="plugin-management-action-description">${escapeHtml(action.description)}</p>
                ${
                  action.status_label
                    ? `<span class="plugin-management-action-status ${escapeHtml(
                        action.status_level
                      )}">${escapeHtml(action.status_label)}</span>`
                    : ''
                }
              </div>
              <button
                type="button"
                class="plugin-action-button ${escapeHtml(action.kind)}"
                data-plugin-action-plugin="${escapeHtml(plugin.id)}"
                data-plugin-action-id="${escapeHtml(action.id)}"
                ${busy || !action.enabled ? 'disabled' : ''}
              >${escapeHtml(action.label)}</button>
            </div>
          `
        })
        .join('')}
      ${
        actionResultText
          ? `<p class="plugin-management-action-result">${escapeHtml(actionResultText)}</p>`
          : ''
      }
    </div>
  `
}

function renderPluginNotificationTestAction(
  plugin: PluginManagementItem,
  busy: boolean,
  resultText: string,
  t: (typeof translations)[LanguageCode]
) {
  if (plugin.kind !== 'notification' || !plugin.capabilities.includes('notification_test')) {
    return ''
  }
  const runnable = plugin.enabled && (plugin.runtime_status === 'running' || plugin.runtime_status === 'starting')
  return `
    <div class="plugin-notification-test-action">
      <button
        type="button"
        class="plugin-action-button secondary"
        data-plugin-notification-test="${escapeHtml(plugin.id)}"
        ${busy || !runnable ? 'disabled' : ''}
      >${escapeHtml(busy ? t.sending : t.testSend)}</button>
      ${
        resultText
          ? `<p class="plugin-notification-test-result">${escapeHtml(t.lastResult)}: ${escapeHtml(resultText)}</p>`
          : ''
      }
    </div>
  `
}

function pluginActionKey(pluginId: string, actionId: string) {
  return `${pluginId}:${actionId}`
}

function renderPluginActions(plugin: PluginManagementItem, busy: boolean, removeText: string) {
  return `
    <div class="plugin-card-actions">
      <button type="button" data-plugin-remove="${escapeHtml(plugin.id)}" ${busy ? 'disabled' : ''}>${escapeHtml(removeText)}</button>
    </div>
  `
}

function renderPluginConfigForm(
  plugin: PluginManagementItem,
  config: Record<string, unknown>,
  busy: boolean,
  resultText: string,
  t: (typeof translations)[LanguageCode]
) {
  if (!plugin.config_schema || plugin.config_schema.length === 0) {
    return ''
  }
  return `
    <form class="plugin-config-form" data-plugin-config-form="${escapeHtml(plugin.id)}">
      <div class="plugin-config-heading">
        <h4>${escapeHtml(t.pluginConfig)}</h4>
        <button type="button" data-plugin-config-save="${escapeHtml(plugin.id)}" ${busy ? 'disabled' : ''}>${escapeHtml(
          busy ? t.saving : t.save
        )}</button>
      </div>
      <div class="plugin-config-fields">
        ${plugin.config_schema
          .map((field) => renderPluginConfigField(plugin.id, field, config[field.key]))
          .join('')}
      </div>
      ${
        resultText
          ? `<p class="plugin-config-result">${escapeHtml(t.lastResult)}: ${escapeHtml(resultText)}</p>`
          : ''
      }
    </form>
  `
}

function renderPluginConfigField(pluginId: string, field: PluginConfigField, value: unknown) {
  const inputId = `plugin-config-${pluginId}-${field.key}`
  const type = field.type === 'secret' ? 'password' : field.type === 'url' ? 'url' : 'text'
  const required = field.required ? 'required' : ''
  if (field.type === 'boolean') {
    return `
      <label class="plugin-config-field boolean" for="${escapeHtml(inputId)}">
        <span>${escapeHtml(field.label)}</span>
        <input id="${escapeHtml(inputId)}" type="checkbox" data-plugin-config-field="${escapeHtml(
          field.key
        )}" ${value === true ? 'checked' : ''}>
      </label>
    `
  }
  if (field.type === 'select') {
    return `
      <label class="plugin-config-field" for="${escapeHtml(inputId)}">
        <span>${escapeHtml(field.label)}</span>
        <select id="${escapeHtml(inputId)}" data-plugin-config-field="${escapeHtml(field.key)}" ${required}>
          ${(field.options ?? [])
            .map(
              (option) =>
                `<option value="${escapeHtml(option)}" ${String(value ?? '') === option ? 'selected' : ''}>${escapeHtml(option)}</option>`
            )
            .join('')}
        </select>
      </label>
    `
  }
  return `
    <label class="plugin-config-field" for="${escapeHtml(inputId)}">
      <span>${escapeHtml(field.label)}</span>
      <input id="${escapeHtml(inputId)}" type="${type}" data-plugin-config-field="${escapeHtml(
        field.key
      )}" value="${escapeHtml(String(value ?? ''))}" ${required}>
    </label>
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

export function translatePluginKind(language: LanguageCode, kind: string) {
  if (kind === 'notification') {
    return language === 'zh-CN' ? '通知插件' : 'notification'
  }
  if (kind === 'status_indicator') {
    return language === 'zh-CN' ? '状态指示插件' : 'status indicator'
  }
  return language === 'zh-CN' ? '工具插件' : 'tool'
}

export function translatePluginCapability(language: LanguageCode, capability: string) {
  const t = translations[language]
  // 能力标签仅用于插件管理展示，真实权限仍由后端能力模型和后续鉴权决定。
  if (capability === 'event_watcher') {
    return t.pluginCapabilityEventWatcher
  }
  if (capability === 'event_consumer') {
    return t.pluginCapabilityEventConsumer
  }
  if (capability === 'approval_handler') {
    return t.pluginCapabilityApprovalHandler
  }
  if (capability === 'notification_test') {
    return t.pluginCapabilityNotificationTest
  }
  if (capability === 'state_consumer') {
    return t.pluginCapabilityStateConsumer
  }
  return capability
}

function isPluginTransitioning(plugin: PluginManagementItem) {
  return plugin.runtime_status === 'starting' || plugin.runtime_status === 'stopping'
}
