import { describe, expect, it } from 'vitest'
import { createPlainRpcRequest, isPlainRpcResponse } from '../remote/plainRpcClient.js'

describe('plain rpc client', () => {
  it('creates request envelope with caller provided id and params', () => {
    const request = createPlainRpcRequest('req_1', 'rpc.ping', { message: 'hello' })

    expect(request).toEqual({
      version: 1,
      type: 'request',
      id: 'req_1',
      method: 'rpc.ping',
      params: { message: 'hello' }
    })
  })

  it('recognizes response envelope with ok flag', () => {
    expect(
      isPlainRpcResponse({
        version: 1,
        type: 'response',
        id: 'req_1',
        ok: true,
        result: { pong: true }
      })
    ).toBe(true)
  })

  it('rejects invalid response envelope shapes', () => {
    expect(isPlainRpcResponse(null)).toBe(false)
    expect(isPlainRpcResponse({ version: 2, type: 'response', id: 'req_1', ok: true })).toBe(false)
    expect(isPlainRpcResponse({ version: 1, type: 'request', id: 'req_1', ok: true })).toBe(false)
    expect(isPlainRpcResponse({ version: 1, type: 'response', id: 1, ok: true })).toBe(false)
    expect(isPlainRpcResponse({ version: 1, type: 'response', id: 'req_1', ok: 'yes' })).toBe(false)
  })
})
