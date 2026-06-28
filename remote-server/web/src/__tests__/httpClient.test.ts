import { afterEach, describe, expect, it, vi } from 'vitest'
import { createMemoryAuthStore } from '../auth/authStore.js'
import { createHttpClient, HttpError } from '../api/httpClient.js'
import { ApiError, unwrapEnvelope } from '../shared/envelope.js'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('api envelope', () => {
  it('unwraps success data and throws business errors', () => {
    expect(unwrapEnvelope({ code: 0, message: 'ok', data: { value: 1 } })).toEqual({ value: 1 })
    expect(() => unwrapEnvelope({ code: 200001, message: '未登录', data: null })).toThrow(ApiError)
  })
})

describe('createHttpClient', () => {
  it('joins baseUrl and sends bearer authorization', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 0, message: 'ok', data: { value: 1 } }), {
        headers: { 'Content-Type': 'application/json' }
      })
    )
    const client = createHttpClient(createMemoryAuthStore('access-token'), 'https://example.com')

    await expect(client.get('/api/v1/devices/list')).resolves.toEqual({ value: 1 })

    expect(fetchMock).toHaveBeenCalledWith(
      'https://example.com/api/v1/devices/list',
      expect.objectContaining({
        method: 'GET',
        headers: expect.any(Headers)
      })
    )
    const headers = fetchMock.mock.calls[0]?.[1]?.headers as Headers
    expect(headers.get('Authorization')).toBe('Bearer access-token')
  })

  it('reports HTTP and JSON boundary errors without leaking parser messages', async () => {
    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(new Response('', { status: 500, statusText: 'Internal Server Error' }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }))
      .mockResolvedValueOnce(new Response('<html></html>', { headers: { 'Content-Type': 'text/html' } }))

    const client = createHttpClient(createMemoryAuthStore())

    await expect(client.get('/http-error')).rejects.toThrow(HttpError)
    await expect(client.get('/empty')).rejects.toThrow('服务端响应为空')
    await expect(client.get('/html')).rejects.toThrow('服务端响应格式错误')
  })

  it('reports network errors with a stable message', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new TypeError('Failed to fetch'))
    const client = createHttpClient(createMemoryAuthStore())

    await expect(client.get('/network-error')).rejects.toThrow('网络请求失败')
  })
})
