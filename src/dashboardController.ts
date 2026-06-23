import type { ListenerToolConfig } from './api'

export function normalizeListenerTools(config: {
  codex_listening_enabled: boolean
  tools?: ListenerToolConfig[]
}) {
  if (config.tools && config.tools.length > 0) {
    return config.tools
  }
  return [
    {
      id: 'codex',
      plugin_id: 'builtin-codex',
      display_name: 'Codex',
      enabled: config.codex_listening_enabled,
      source: 'builtin',
      icon_url: null
    }
  ]
}

export function portFromUrl(url: string) {
  try {
    return new URL(url).port || '-'
  } catch {
    return '-'
  }
}
