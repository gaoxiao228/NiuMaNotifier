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

  it('uses a stable message key for client-side missing data errors', () => {
    expect(() => unwrapEnvelope({ code: 0, message: 'ok', data: null })).toThrow('api_error_missing_data')
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

  it('preserves business envelope errors from non-2xx responses', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 200001, message: '未登录', data: null }), {
        status: 401,
        headers: { 'Content-Type': 'application/json' }
      })
    )
    const client = createHttpClient(createMemoryAuthStore())

    await expect(client.get('/unauthorized')).rejects.toMatchObject({ code: 200001, message: '未登录' })
  })

  it('reports HTTP and JSON boundary errors with stable message keys', async () => {
    vi.spyOn(globalThis, 'fetch')
      .mockResolvedValueOnce(new Response('', { status: 500, statusText: 'Internal Server Error' }))
      .mockResolvedValueOnce(new Response(null, { status: 204 }))
      .mockResolvedValueOnce(new Response('<html></html>', { headers: { 'Content-Type': 'text/html' } }))

    const client = createHttpClient(createMemoryAuthStore())

    await expect(client.get('/http-error')).rejects.toMatchObject({ messageKey: 'api_error_http' })
    await expect(client.get('/empty')).rejects.toMatchObject({ messageKey: 'api_error_empty_response' })
    await expect(client.get('/html')).rejects.toMatchObject({ messageKey: 'api_error_invalid_json' })
  })

  it('reports network errors with a stable message', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new TypeError('Failed to fetch'))
    const client = createHttpClient(createMemoryAuthStore())

    await expect(client.get('/network-error')).rejects.toMatchObject({ messageKey: 'api_error_network' })
  })
})
