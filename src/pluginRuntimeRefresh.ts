export type PluginRuntimeRefreshTimer = {
  setInterval(callback: () => void, delayMs: number): number
  clearInterval(timerId: number): void
}

export type PluginRuntimeRefreshController = {
  start(): void
  stop(): void
}

type PluginRuntimeRefreshOptions = {
  intervalMs: number
  refresh: () => Promise<void>
  timer?: PluginRuntimeRefreshTimer
  onError?: (error: unknown) => void
}

const browserTimer: PluginRuntimeRefreshTimer = {
  setInterval: (callback, delayMs) => window.setInterval(callback, delayMs),
  clearInterval: (timerId) => window.clearInterval(timerId)
}

export function createPluginRuntimeRefresh(options: PluginRuntimeRefreshOptions) {
  const timer = options.timer ?? browserTimer
  let timerId: number | undefined
  let refreshInFlight = false

  async function runRefresh() {
    if (refreshInFlight) {
      return
    }
    refreshInFlight = true
    try {
      await options.refresh()
    } catch (error) {
      // 运行态刷新只是 UI 快照同步，失败时保留现有界面并等待下一轮。
      options.onError?.(error)
    } finally {
      refreshInFlight = false
    }
  }

  return {
    start() {
      if (timerId !== undefined) {
        return
      }
      timerId = timer.setInterval(() => {
        void runRefresh()
      }, options.intervalMs)
    },
    stop() {
      if (timerId === undefined) {
        return
      }
      timer.clearInterval(timerId)
      timerId = undefined
    }
  } satisfies PluginRuntimeRefreshController
}
