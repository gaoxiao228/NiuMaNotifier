import { pluginManagementSnapshotsEqual } from '../src/pluginSnapshot'
import type { PluginManagementItem } from '../src/api'

function plugin(overrides: Partial<PluginManagementItem> = {}): PluginManagementItem {
  return {
    id: 'builtin-bark',
    kind: 'notification',
    tool_id: null,
    display_name: 'Bark',
    version: '0.1.0',
    source: 'builtin',
    capabilities: ['event_consumer', 'notification_test'],
    enabled: true,
    runtime_status: 'running',
    last_error: null,
    icon_url: '/assets/bark-icon.png',
    config_schema: [],
    install_path: null,
    ...overrides
  }
}

const current = [plugin()]

if (!pluginManagementSnapshotsEqual(current, [plugin()])) {
  throw new Error('内容相同的插件快照应视为未变化')
}

if (pluginManagementSnapshotsEqual(current, [plugin({ runtime_status: 'failed' })])) {
  throw new Error('插件运行态变化时应视为快照变化')
}

if (pluginManagementSnapshotsEqual(current, [plugin({ last_error: '启动失败' })])) {
  throw new Error('插件错误信息变化时应视为快照变化')
}

if (
  pluginManagementSnapshotsEqual(current, [
    plugin(),
    plugin({ id: 'builtin-ntfy', display_name: 'ntfy', icon_url: '/assets/ntfy-logo.svg' })
  ])
) {
  throw new Error('插件数量变化时应视为快照变化')
}
