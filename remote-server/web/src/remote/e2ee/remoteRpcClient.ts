import { createRpcRequest, isRpcResponse } from './rpcEnvelope.js'
import type { RpcResponse } from './types.js'

type Pending = {
  resolve(value: unknown): void
  reject(error: Error): void
  timer: ReturnType<typeof setTimeout>
}

export function createRemoteRpcClient(options: {
  timeoutMs: number
  sendEncrypted(payload: unknown): Promise<void>
}) {
  const pending = new Map<string, Pending>()
  let lastRequestId = ''

  function registerPending(id: string, entry: Pending) {
    if (pending.has(id)) throw new Error('duplicate request id')
    pending.set(id, entry)
  }

  function handleResponse(value: unknown) {
    if (!isRpcResponse(value)) return false
    const item = pending.get(value.id)
    if (!item) return false
    clearTimeout(item.timer)
    pending.delete(value.id)
    if (value.ok) item.resolve(value.result)
    else item.reject(new Error(value.error?.message ?? 'remote rpc failed'))
    return true
  }

  return {
    async request(method: string, params: Record<string, unknown>) {
      const id = `req_${crypto.randomUUID()}`
      lastRequestId = id
      const request = createRpcRequest(id, method, params)
      const promise = new Promise<unknown>((resolve, reject) => {
        // 每个请求必须有明确超时，避免远端断线时 pending map 无限增长。
        const timer = setTimeout(() => {
          pending.delete(id)
          reject(new Error('remote rpc timeout'))
        }, options.timeoutMs)
        registerPending(id, { resolve, reject, timer })
      })
      await options.sendEncrypted(request)
      return promise
    },

    handleResponse,

    registerPendingForTest(id: string) {
      registerPending(id, {
        resolve: () => {},
        reject: () => {},
        timer: setTimeout(() => {}, 1000)
      })
    },

    resolveForTest(response: RpcResponse) {
      return handleResponse(response)
    },

    lastRequestIdForTest() {
      return lastRequestId
    }
  }
}
