import type { PluginManagementItem } from '../src/api'
import {
  hasPluginReachedTransitionTarget,
  isPluginTransitioning,
  mergePendingPluginTransition
} from '../src/pluginTransition'

function plugin(overrides: Partial<PluginManagementItem>): PluginManagementItem {
  return {
    id: 'niuma-plugin-demo',
    tool_id: 'demo_tool',
    display_name: 'Demo Tool',
    version: '0.1.0',
    source: 'external',
    enabled: false,
    runtime_status: 'stopped',
    last_error: null,
    icon_url: null,
    install_path: '/tmp/niuma-plugin-demo',
    ...overrides
  }
}

const staleEnableSnapshot = plugin({ enabled: false, runtime_status: 'stopped' })
const optimisticEnable = mergePendingPluginTransition(staleEnableSnapshot, {
  pluginId: 'niuma-plugin-demo',
  desiredEnabled: true,
  optimisticStatus: 'starting'
})

if (!optimisticEnable.enabled || optimisticEnable.runtime_status !== 'starting') {
  throw new Error('启用插件时，旧 stopped 快照不应覆盖前端 starting 过渡态')
}

if (hasPluginReachedTransitionTarget(staleEnableSnapshot, true)) {
  throw new Error('启用目标下，enabled=false + stopped 不能被视为最终状态')
}

const runningSnapshot = plugin({ enabled: true, runtime_status: 'running' })
const mergedRunning = mergePendingPluginTransition(runningSnapshot, {
  pluginId: 'niuma-plugin-demo',
  desiredEnabled: true,
  optimisticStatus: 'starting'
})

if (mergedRunning.runtime_status !== 'running') {
  throw new Error('启用目标达成后应显示后端 running 状态')
}

const staleDisableSnapshot = plugin({ enabled: true, runtime_status: 'running' })
const optimisticDisable = mergePendingPluginTransition(staleDisableSnapshot, {
  pluginId: 'niuma-plugin-demo',
  desiredEnabled: false,
  optimisticStatus: 'stopping'
})

if (optimisticDisable.enabled || optimisticDisable.runtime_status !== 'stopping') {
  throw new Error('禁用插件时，旧 running 快照不应覆盖前端 stopping 过渡态')
}

if (!isPluginTransitioning(plugin({ runtime_status: 'starting' }))) {
  throw new Error('starting 应被识别为插件过渡态')
}

if (isPluginTransitioning(plugin({ runtime_status: 'running' }))) {
  throw new Error('running 不应被识别为插件过渡态')
}
