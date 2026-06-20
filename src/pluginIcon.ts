import type { PluginManagementItem } from './api'
import { escapeHtml } from './viewUtils'

type PluginIconSource = Pick<PluginManagementItem, 'display_name' | 'icon_url'>

export function renderPluginIcon(plugin: PluginIconSource) {
  const label = plugin.display_name || 'Plugin'
  const iconUrl = plugin.icon_url?.trim()
  if (iconUrl) {
    return `<span class="plugin-icon image"><img src="${escapeHtml(iconUrl)}" alt="${escapeHtml(label)}"></span>`
  }

  // 没有图标的外部插件也保留稳定视觉锚点，避免插件列表布局跳动。
  return `<span class="plugin-icon fallback" aria-label="${escapeHtml(label)}">${escapeHtml(
    fallbackInitial(label)
  )}</span>`
}

function fallbackInitial(label: string) {
  const firstChar = Array.from(label.trim())[0]
  return firstChar ? firstChar.toUpperCase() : '?'
}
