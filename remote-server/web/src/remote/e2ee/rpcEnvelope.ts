import type { RpcRequest, RpcResponse } from './types.js'

export function createRpcRequest(id: string, method: string, params: Record<string, unknown>): RpcRequest {
  return {
    version: 1,
    type: 'request',
    id,
    method,
    params
  }
}

export function isRpcResponse(value: unknown): value is RpcResponse {
  const item = value as RpcResponse
  return (
    item?.version === 1 &&
    item?.type === 'response' &&
    typeof item.id === 'string' &&
    typeof item.ok === 'boolean'
  )
}
