import { createRemoteAgentStatusRefresh } from '../src/remoteAgentStatusRefresh'

class FakeTimer {
  callback: (() => void) | null = null
  delayMs: number | null = null
  cleared = false

  setInterval(callback: () => void, delayMs: number) {
    if (this.callback) {
      throw new Error('远程状态刷新轮询不应重复启动')
    }
    this.callback = callback
    this.delayMs = delayMs
    this.cleared = false
    return 1
  }

  clearInterval(timerId: number) {
    if (timerId !== 1) {
      throw new Error('远程状态刷新应清理已创建的定时器')
    }
    this.callback = null
    this.cleared = true
  }

  tick() {
    this.callback?.()
  }
}

async function flushMicrotasks() {
  await Promise.resolve()
  await Promise.resolve()
}

async function main() {
  const timer = new FakeTimer()
  let refreshActive = false
  let refreshCalls = 0
  const pendingRefreshes: Array<() => void> = []
  const getRefreshCalls = () => refreshCalls

  const controller = createRemoteAgentStatusRefresh({
    intervalMs: 1_000,
    timer,
    isActive: () => refreshActive,
    refresh: async () => {
      refreshCalls += 1
      await new Promise<void>((resolve) => pendingRefreshes.push(resolve))
    }
  })

  controller.start()
  controller.start()

  if (timer.delayMs !== 1_000) {
    throw new Error('远程状态刷新轮询应使用指定间隔')
  }

  await flushMicrotasks()
  if (getRefreshCalls() !== 0) {
    throw new Error('远程访问页不可见时，启动轮询不应立即刷新远程状态')
  }

  timer.tick()
  if (getRefreshCalls() !== 0) {
    throw new Error('远程访问页不可见时不应刷新远程状态')
  }

  refreshActive = true
  controller.stop()
  controller.start()
  await flushMicrotasks()
  if (getRefreshCalls() !== 1) {
    throw new Error('远程访问页可见时，启动轮询应立即刷新远程状态')
  }

  timer.tick()
  timer.tick()
  if (getRefreshCalls() !== 1) {
    throw new Error('上一次远程状态刷新未完成时，不应并发发起新刷新')
  }

  pendingRefreshes.shift()?.()
  await flushMicrotasks()

  timer.tick()
  if (getRefreshCalls() !== 2) {
    throw new Error('远程状态刷新完成后，下一个轮询周期应继续刷新')
  }

  controller.stop()
  timer.tick()
  if (!timer.cleared || getRefreshCalls() !== 2) {
    throw new Error('停止远程状态刷新后，不应继续触发刷新')
  }
}

void main()
