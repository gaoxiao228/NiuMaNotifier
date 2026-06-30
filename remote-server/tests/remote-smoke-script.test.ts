import { describe, expect, it, vi } from 'vitest'
import {
  buildSmokeConfig,
  expectEnvelopeOk,
  runRemoteSmokeChecks,
  type SmokeFetch
} from '../scripts/remote-smoke.mjs'

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    headers: { 'Content-Type': 'application/json', ...(init.headers ?? {}) },
    status: init.status ?? 200
  })
}

describe('remote smoke script', () => {
  it('uses project-specific local endpoints by default', () => {
    const config = buildSmokeConfig({})

    expect(config.remoteServerUrl).toBe('http://127.0.0.1:27880')
    expect(config.remoteClientWebUrl).toBe('http://127.0.0.1:27883')
    expect(config.corsOrigin).toBe('http://localhost:27883')
  })

  it('rejects non-ok api envelopes', async () => {
    await expect(expectEnvelopeOk(jsonResponse({ code: 200001, message: '未登录', data: null }), 'auth'))
      .rejects.toThrow('auth failed: 200001 未登录')
  })

  it('checks health, cors, web client, user login, device list, and admin boundary', async () => {
    const fetchMock = vi.fn<SmokeFetch>(async (url, init) => {
      const method = init?.method ?? 'GET'
      const textUrl = String(url)

      if (textUrl === 'http://127.0.0.1:27880/api/v1/health') {
        return jsonResponse({ code: 0, message: 'ok', data: { status: 'ok' } })
      }
      if (textUrl === 'http://127.0.0.1:27880/api/v1/auth/login' && method === 'OPTIONS') {
        return new Response(null, {
          status: 204,
          headers: { 'access-control-allow-origin': 'http://localhost:27883' }
        })
      }
      if (textUrl === 'http://127.0.0.1:27883' || textUrl === 'http://127.0.0.1:27883/') {
        return new Response('<html><title>NiuMa Remote Client</title></html>')
      }
      if (textUrl === 'http://127.0.0.1:27880/api/v1/auth/login' && method === 'POST') {
        return jsonResponse({
          code: 0,
          message: 'ok',
          data: {
            access_token: 'access-user',
            refresh_token: 'refresh-user',
            expires_at: '2026-06-30T00:00:00Z',
            user: { id: 'usr_1', email: 'user@example.com', role: 'user', status: 'active' }
          }
        })
      }
      if (textUrl === 'http://127.0.0.1:27880/api/v1/devices/list') {
        expect(new Headers(init?.headers).get('Authorization')).toBe('Bearer access-user')
        return jsonResponse({ code: 0, message: 'ok', data: { list: [] } })
      }
      if (textUrl === 'http://127.0.0.1:27880/api/v1/admin/auth/login' && method === 'POST') {
        return jsonResponse({ code: 230401, message: '需要管理员权限', data: null })
      }

      throw new Error(`Unexpected smoke request: ${method} ${textUrl}`)
    })

    await expect(runRemoteSmokeChecks({
      fetchImpl: fetchMock,
      env: {
        SMOKE_USER_EMAIL: 'user@example.com',
        SMOKE_USER_PASSWORD: '11111111',
        SMOKE_EXPECT_USER_ADMIN_FORBIDDEN: 'true'
      }
    })).resolves.toEqual([
      'remote-server health ok',
      'cors preflight ok',
      'remote-client-web reachable',
      'user login ok',
      'device list ok',
      'normal user admin boundary ok'
    ])
  })
})
