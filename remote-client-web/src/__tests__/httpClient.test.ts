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

  it('notifies auth expiration for token business errors', async () => {
    const onAuthExpired = vi.fn()
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code: 200003, message: 'Token 已过期', data: null }), { status: 401 })
    )
    const client = createHttpClient({ baseUrl: '', onAuthExpired })

    await expect(client.get('/api/v1/auth/me')).rejects.toMatchObject({
      code: 200003,
      status: 401,
      message: 'Token 已过期'
    })
    expect(onAuthExpired).toHaveBeenCalledTimes(1)
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
