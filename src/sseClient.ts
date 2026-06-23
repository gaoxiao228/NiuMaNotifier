export interface StateStreamClientOptions<TState> {
  url: string
  fallbackIntervalMs: number
  onConnected: (connected: boolean) => void
  onState: (state: TState) => void
  onFallback: () => Promise<void>
  onFallbackError: (error: unknown) => void
}

export interface StateStreamClient {
  start: () => void
  stop: () => void
}

export function createStateStreamClient<TState>(
  options: StateStreamClientOptions<TState>
): StateStreamClient {
  let stream: EventSource | undefined
  let fallbackTimer: number | undefined

  function startFallbackPolling() {
    if (fallbackTimer !== undefined) {
      return
    }
    fallbackTimer = window.setInterval(() => {
      options.onFallback().catch(options.onFallbackError)
    }, options.fallbackIntervalMs)
  }

  function stopFallbackPolling() {
    if (fallbackTimer === undefined) {
      return
    }
    window.clearInterval(fallbackTimer)
    fallbackTimer = undefined
  }

  return {
    start() {
      stream?.close()
      stream = new EventSource(options.url)
      stream.onopen = () => {
        options.onConnected(true)
      }
      stream.addEventListener('state', (message) => {
        options.onState(JSON.parse((message as MessageEvent<string>).data) as TState)
        stopFallbackPolling()
      })
      stream.onerror = () => {
        options.onConnected(false)
        startFallbackPolling()
      }
    },
    stop() {
      stream?.close()
      stream = undefined
      stopFallbackPolling()
      options.onConnected(false)
    }
  }
}
