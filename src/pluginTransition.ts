import type { PluginManagementItem, PluginRuntimeStatus } from './api'

export type PluginTransitionTarget = {
  pluginId: string
  desiredEnabled: boolean
  optimisticStatus: PluginRuntimeStatus
}

export function mergePendingPluginTransition(
  plugin: PluginManagementItem,
  transition: PluginTransitionTarget | null
) {
  if (!transition || plugin.id !== transition.pluginId) {
    return plugin
  }
  if (hasPluginReachedTransitionTarget(plugin, transition.desiredEnabled)) {
    return plugin
  }
  // 配置保存到运行时写入状态之间存在短暂窗口；保留乐观态避免 UI 闪回旧状态。
  return {
    ...plugin,
    enabled: transition.desiredEnabled,
    runtime_status: transition.optimisticStatus
  }
}

export function hasPluginReachedTransitionTarget(
  plugin: PluginManagementItem | undefined,
  desiredEnabled: boolean
) {
  if (!plugin || plugin.enabled !== desiredEnabled) {
    return false
  }
  const stableStatuses: PluginRuntimeStatus[] = desiredEnabled
    ? ['running', 'failed']
    : ['stopped', 'failed']
  return stableStatuses.includes(plugin.runtime_status)
}

export function isPluginTransitioning(plugin: PluginManagementItem | undefined) {
  return plugin?.runtime_status === 'starting' || plugin?.runtime_status === 'stopping'
}
