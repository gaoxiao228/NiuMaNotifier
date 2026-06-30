export type RemoteAgentStatusRefreshTimer = {
  setInterval(callback: () => void, delayMs: number): number
  clearInterval(timerId: number): void
}

export type RemoteAgentStatusRefreshController = {
  start(): void
  stop(): void
}

type RemoteAgentStatusRefreshOptions = {
  intervalMs: number
  refresh: () => Promise<void>
  isActive?: () => boolean
  timer?: RemoteAgentStatusRefreshTimer
  onError?: (error: unknown) => void
}

const browserTimer: RemoteAgentStatusRefreshTimer = {
  setInterval: (callback, delayMs) => window.setInterval(callback, delayMs),
  clearInterval: (timerId) => window.clearInterval(timerId)
}

export function createRemoteAgentStatusRefresh(options: RemoteAgentStatusRefreshOptions) {
  const timer = options.timer ?? browserTimer
  let timerId: number | undefined
  let refreshInFlight = false

  async function runRefresh() {
    if (options.isActive && !options.isActive()) {
      return
    }
    if (refreshInFlight) {
      return
    }
    refreshInFlight = true
    try {
      await options.refresh()
    } catch (error) {
      // 远程状态是设置页附加信息，刷新失败时保留现有快照等待下一轮。
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
      // 打开远程访问页时立即同步一次，避免连接状态变化后还要等下一轮定时器。
      void runRefresh()
    },
    stop() {
      if (timerId === undefined) {
        return
      }
      timer.clearInterval(timerId)
      timerId = undefined
    }
  } satisfies RemoteAgentStatusRefreshController
}
