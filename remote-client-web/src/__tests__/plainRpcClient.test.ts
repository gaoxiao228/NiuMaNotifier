import { describe, expect, it, vi } from 'vitest'
import { createPlainRpcClient, createPlainRpcRequest, isPlainRpcResponse } from '../remote/plainRpcClient.js'

describe('plain rpc client', () => {
  it('creates request envelope with caller provided id and params', () => {
    const request = createPlainRpcRequest('req_1', 'rpc.ping', { message: 'hello' })

    expect(request).toEqual({
      version: 1,
      type: 'request',
      transport: {
        kind: 'relay'
      },
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

  it('dispatches notification messages without resolving a request', () => {
    const send = vi.fn()
    const onNotification = vi.fn()
    const client = createPlainRpcClient({
      timeoutMs: 1000,
      send,
      onNotification
    })

    client.handle({
      version: 1,
      type: 'notification',
      transport: {
        kind: 'relay'
      },
      method: 'local_api.stream.event',
      params: { stream_id: 'stream_1' }
    }, 'relay')

    expect(onNotification).toHaveBeenCalledWith({
      method: 'local_api.stream.event',
      params: { stream_id: 'stream_1' },
      observedTransport: 'relay',
      declaredTransport: 'relay'
    })
    expect(send).not.toHaveBeenCalled()
  })

  it('rejects invalid response envelope shapes', () => {
    expect(isPlainRpcResponse(null)).toBe(false)
    expect(isPlainRpcResponse({ version: 2, type: 'response', id: 'req_1', ok: true })).toBe(false)
    expect(isPlainRpcResponse({ version: 1, type: 'request', id: 'req_1', ok: true })).toBe(false)
    expect(isPlainRpcResponse({ version: 1, type: 'response', id: 1, ok: true })).toBe(false)
    expect(isPlainRpcResponse({ version: 1, type: 'response', id: 'req_1', ok: 'yes' })).toBe(false)
  })

  it('sends a pending request and resolves matching success response', async () => {
    const send = vi.fn()
    const client = createPlainRpcClient({ timeoutMs: 1000, send })

    const resultPromise = client.request('state.get', { scope: 'runtime' })

    expect(send).toHaveBeenCalledWith({
      version: 1,
      type: 'request',
      transport: {
        kind: 'relay'
      },
      id: 'rpc_1',
      method: 'state.get',
      params: { scope: 'runtime' }
    })

    client.handle({
      version: 1,
      type: 'response',
      id: 'rpc_1',
      ok: true,
      result: { state: 'ready' }
    })

    await expect(resultPromise).resolves.toEqual({ state: 'ready' })
  })

  it('rejects matching error response', async () => {
    const client = createPlainRpcClient({ timeoutMs: 1000, send: vi.fn() })

    const resultPromise = client.request('rpc.ping')
    client.handle({
      version: 1,
      type: 'response',
      id: 'rpc_1',
      ok: false,
      error: { code: 'REMOTE_ERROR', message: 'failed' }
    })

    await expect(resultPromise).rejects.toMatchObject({
      message: 'REMOTE_ERROR: failed'
    })
  })

  it('keeps remote error details when rejecting an error response', async () => {
    const client = createPlainRpcClient({ timeoutMs: 1000, send: vi.fn() })

    const resultPromise = client.request('rpc.ping')
    client.handle({
      version: 1,
      type: 'response',
      id: 'rpc_1',
      ok: false,
      error: { code: 'method_not_found', message: 'unknown RPC method: demo.missing' }
    })

    await expect(resultPromise).rejects.toMatchObject({
      message: 'method_not_found: unknown RPC method: demo.missing'
    })
  })

  it('rejects pending requests on timeout and close', async () => {
    vi.useFakeTimers()
    try {
      const timeoutClient = createPlainRpcClient({ timeoutMs: 25, send: vi.fn() })
      const timeoutPromise = timeoutClient.request('session.list')

      vi.advanceTimersByTime(25)
      await expect(timeoutPromise).rejects.toMatchObject({ message: 'Plain RPC request timed out' })

      const closeClient = createPlainRpcClient({ timeoutMs: 1000, send: vi.fn() })
      const closePromise = closeClient.request('state.get')
      closeClient.close()

      await expect(closePromise).rejects.toMatchObject({ message: 'Plain RPC client closed' })
    } finally {
      vi.useRealTimers()
    }
  })

  it('clears a pending request immediately when send throws', async () => {
    vi.useFakeTimers()
    try {
      const client = createPlainRpcClient({
        timeoutMs: 1000,
        send: () => {
          throw new Error('Relay websocket is not open')
        }
      })

      const resultPromise = client.request('rpc.ping')

      await expect(resultPromise).rejects.toMatchObject({ message: 'Relay websocket is not open' })
      expect(vi.getTimerCount()).toBe(0)
    } finally {
      vi.useRealTimers()
    }
  })
})
