import { afterEach, describe, expect, it, vi } from 'vitest'

import { ApiError, createHttpClient } from '../api/httpClient.js'

afterEach(() => {
  vi.restoreAllMocks()
})

describe('createHttpClient', () => {
  it('unwraps successful envelopes', async () => {
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 0, message: 'ok', data: { value: 1 } }))
    )
    const client = createHttpClient({ baseUrl: '' })

    await expect(client.get('/api/v1/ping')).resolves.toEqual({ value: 1 })
  })

  it('injects bearer authorization when access token exists', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 0, message: 'ok', data: { list: [] } }))
    )
    const client = createHttpClient({
      baseUrl: 'https://remote.example.com/',
      getAccessToken: () => 'access-token'
    })

    await client.get('/api/v1/devices/list')

    const headers = fetchMock.mock.calls[0]?.[1]?.headers as Headers
    expect(headers.get('Authorization')).toBe('Bearer access-token')
  })

  it.each([
    { code: 200001, message: '未登录' },
    { code: 200002, message: 'Token 无效' },
    { code: 200003, message: 'Token 已过期' }
  ])('notifies auth expiration for token business error $code', async ({ code, message }) => {
    const onAuthExpired = vi.fn()
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code, message, data: null }))
    )
    const client = createHttpClient({ baseUrl: '', onAuthExpired })

    await expect(client.get('/api/v1/auth/me')).rejects.toMatchObject({
      code,
      status: 200,
      message
    })
    expect(onAuthExpired).toHaveBeenCalledTimes(1)
  })

  it('does not notify auth expiration for normal business errors', async () => {
    const onAuthExpired = vi.fn()
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 100101, message: '邮箱或密码错误', data: null }))
    )
    const client = createHttpClient({ baseUrl: '', onAuthExpired })

    await expect(client.post('/api/v1/auth/login', { email: 'user@example.com' })).rejects.toMatchObject({
      code: 100101,
      status: 200,
      message: '邮箱或密码错误'
    })
    expect(onAuthExpired).not.toHaveBeenCalled()
  })

  it('throws ApiError with empty response message key', async () => {
    vi.spyOn(globalThis, 'fetch').mockImplementation(async () => new Response(''))
    const client = createHttpClient({ baseUrl: '' })

    await expect(client.get('/empty')).rejects.toBeInstanceOf(ApiError)
    await expect(client.get('/empty')).rejects.toMatchObject({
      code: -1,
      status: 200,
      messageKey: 'api_error_empty_response'
    })
  })

  it.each([
    { name: 'non JSON body', body: '<html></html>' },
    { name: 'invalid envelope shape', body: JSON.stringify({ code: 0, message: 'ok' }) }
  ])('throws ApiError with invalid JSON message key for $name', async ({ body }) => {
    vi.spyOn(globalThis, 'fetch').mockImplementation(async () => new Response(body))
    const client = createHttpClient({ baseUrl: '' })

    await expect(client.get('/invalid-json')).rejects.toBeInstanceOf(ApiError)
    await expect(client.get('/invalid-json')).rejects.toMatchObject({
      code: -1,
      status: 200,
      messageKey: 'api_error_invalid_json'
    })
  })

  it('throws ApiError for non-2xx HTTP responses without a valid envelope', async () => {
    vi.spyOn(globalThis, 'fetch').mockImplementation(async () => new Response('Internal Server Error', { status: 500 }))
    const client = createHttpClient({ baseUrl: '' })

    await expect(client.get('/http-error')).rejects.toBeInstanceOf(ApiError)
    await expect(client.get('/http-error')).rejects.toMatchObject({
      code: 500,
      status: 500,
      messageKey: 'api_error_http'
    })
  })

  it('wraps network failures in ApiError', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new TypeError('Failed to fetch'))
    const client = createHttpClient({ baseUrl: '' })

    await expect(client.get('/network-error')).rejects.toBeInstanceOf(ApiError)
    await expect(client.get('/network-error')).rejects.toMatchObject({
      code: -1,
      status: 0,
      messageKey: 'api_error_network'
    })
  })

  it('joins baseUrl and leading slash paths consistently', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockImplementation(async () => {
      return new Response(JSON.stringify({ code: 0, message: 'ok', data: {} }))
    })
    const withSlash = createHttpClient({ baseUrl: 'https://remote.example.com/' })
    const withoutSlash = createHttpClient({ baseUrl: 'https://remote.example.com' })

    await withSlash.get('/api/v1/devices/list')
    await withoutSlash.get('/api/v1/connections/ice-config')

    expect(fetchMock.mock.calls[0]?.[0]).toBe('https://remote.example.com/api/v1/devices/list')
    expect(fetchMock.mock.calls[1]?.[0]).toBe('https://remote.example.com/api/v1/connections/ice-config')
  })
})
