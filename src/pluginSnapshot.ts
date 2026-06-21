import type { PluginConfigField, PluginManagementItem } from './api'

type ComparablePlugin = {
  id: string
  kind: string | null
  tool_id: string | null
  display_name: string
  version: string
  source: string
  capabilities: string[]
  enabled: boolean
  runtime_status: string
  last_error: string | null
  icon_url: string | null
  config_schema: ComparableConfigField[]
  install_path: string | null
}

type ComparableConfigField = {
  key: string
  type: string
  label: string
  required: boolean
  default: unknown
  options: string[]
}

export function pluginManagementSnapshotsEqual(
  left: PluginManagementItem[],
  right: PluginManagementItem[]
) {
  return stableSnapshot(left) === stableSnapshot(right)
}

function stableSnapshot(plugins: PluginManagementItem[]) {
  return JSON.stringify(plugins.map(normalizePluginForComparison))
}

function normalizePluginForComparison(plugin: PluginManagementItem): ComparablePlugin {
  return {
    id: plugin.id,
    kind: plugin.kind ?? null,
    tool_id: plugin.tool_id,
    display_name: plugin.display_name,
    version: plugin.version,
    source: plugin.source,
    capabilities: [...plugin.capabilities],
    enabled: plugin.enabled,
    runtime_status: plugin.runtime_status,
    last_error: plugin.last_error,
    icon_url: plugin.icon_url,
    config_schema: plugin.config_schema.map(normalizeConfigFieldForComparison),
    install_path: plugin.install_path
  }
}

function normalizeConfigFieldForComparison(field: PluginConfigField): ComparableConfigField {
  return {
    key: field.key,
    type: field.type,
    label: field.label,
    required: field.required ?? false,
    default: field.default ?? null,
    options: [...(field.options ?? [])]
  }
}
