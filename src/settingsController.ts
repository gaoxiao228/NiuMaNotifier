import type { PluginManagementItem } from './api'

export function notificationPlugins(plugins: PluginManagementItem[]) {
  return plugins.filter((plugin) => plugin.kind === 'notification')
}
