export type PlainRpcRequest = {
  version: 1
  type: 'request'
  id: string
  method: string
  params: unknown
}

export type PlainRpcResponse = {
  version: 1
  type: 'response'
  id: string
  ok: boolean
  result?: unknown
  error?: unknown
}

export type PlainRpcNotification = {
  version: 1
  type: 'notification'
  method: string
  params: unknown
}

export type PlainRpcClientOptions = {
  timeoutMs: number
  send(value: PlainRpcRequest): void
  onNotification?: (notification: { method: string; params: unknown }) => void
}

export type PlainRpcClient = {
  request(method: string, params?: unknown): Promise<unknown>
  handle(value: unknown): void
  close(): void
}

type PendingRequest = {
  timer: ReturnType<typeof setTimeout>
  resolve(value: unknown): void
  reject(error: Error): void
}

export function createPlainRpcRequest(id: string, method: string, params: unknown): PlainRpcRequest {
  return {
    version: 1,
    type: 'request',
    id,
    method,
    params
  }
}

export function isPlainRpcResponse(value: unknown): value is PlainRpcResponse {
  // 这里只做 relay MVP 所需的 envelope 基础识别，业务结果结构由调用方判断。
  if (value === null || typeof value !== 'object') {
    return false
  }

  const item = value as Partial<PlainRpcResponse>
  return (
    item.version === 1 &&
    item.type === 'response' &&
    typeof item.id === 'string' &&
    typeof item.ok === 'boolean'
  )
}

export function isPlainRpcNotification(value: unknown): value is PlainRpcNotification {
  if (value === null || typeof value !== 'object') {
    return false
  }

  const item = value as Partial<PlainRpcNotification>
  return item.version === 1 && item.type === 'notification' && typeof item.method === 'string'
}

export function createPlainRpcClient(options: PlainRpcClientOptions): PlainRpcClient {
  let nextId = 1
  let closed = false
  const pending = new Map<string, PendingRequest>()

  function rejectPending(id: string, error: Error) {
    const item = pending.get(id)
    if (!item) return
    clearTimeout(item.timer)
    pending.delete(id)
    item.reject(error)
  }

  return {
    request(method, params = {}) {
      if (closed) return Promise.reject(new Error('Plain RPC client closed'))

      const id = `rpc_${nextId}`
      nextId += 1

      return new Promise((resolve, reject) => {
        const timer = setTimeout(() => {
          rejectPending(id, new Error('Plain RPC request timed out'))
        }, options.timeoutMs)
        pending.set(id, { timer, resolve, reject })
        try {
          options.send(createPlainRpcRequest(id, method, params))
        } catch (err) {
          // send 失败代表请求不会离开本地，立即清理 pending，避免继续等 timeout。
          rejectPending(id, err instanceof Error ? err : new Error('Plain RPC send failed'))
        }
      })
    },
    handle(value) {
      if (closed) return
      if (isPlainRpcNotification(value)) {
        options.onNotification?.({ method: value.method, params: value.params })
        return
      }
      if (!isPlainRpcResponse(value)) return

      const item = pending.get(value.id)
      if (!item) return
      clearTimeout(item.timer)
      pending.delete(value.id)

      if (value.ok) {
        item.resolve(value.result)
        return
      }

      item.reject(new Error('Plain RPC request failed'))
    },
    close() {
      if (closed) return
      closed = true
      // 关闭连接时主动拒绝所有 pending，避免页面卸载后 Promise 悬挂。
      for (const [id] of pending) {
        rejectPending(id, new Error('Plain RPC client closed'))
      }
    }
  }
}
