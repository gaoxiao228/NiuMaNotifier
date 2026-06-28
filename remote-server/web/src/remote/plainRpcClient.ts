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
