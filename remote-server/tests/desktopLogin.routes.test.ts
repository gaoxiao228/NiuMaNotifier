import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { registerDesktopLoginRoutes } from '../src/modules/desktopLogin/desktopLogin.routes.js'
import { ErrorCode } from '../src/shared/errors.js'
import { apiFailure } from '../src/shared/response.js'

describe('desktop login routes', () => {
  it('serves browser login page for desktop binding requests', async () => {
    const previousEnv = { ...process.env }
    Object.assign(process.env, {
      DATABASE_URL: 'postgres://niuma:pw@postgres:5432/niuma_remote',
      REDIS_URL: 'redis://redis:6379',
      JWT_PRIVATE_KEY: 'private',
      JWT_PUBLIC_KEY: 'public',
      TOKEN_PEPPER: 'pepper'
    })
    const app = buildApp({ registerDesktopLoginRoutes })

    const response = await app.inject({
      method: 'GET',
      url: '/desktop-login?request_id=dlr_123'
    })

    process.env = previousEnv
    await app.close()
    expect(response.statusCode).toBe(200)
    expect(response.headers['content-type']).toContain('text/html')
    expect(response.body).toContain('desktop-login-form')
    expect(response.body).toContain('dlr_123')
  })

  it('validates start request', async () => {
    const app = buildApp({
      registerDesktopLoginRoutes: async (instance) => {
        instance.post('/api/v1/desktop-login/start', async () =>
          apiFailure(ErrorCode.BUSINESS_VALIDATION_FAILED, 'device_name不能为空')
        )
      }
    })
    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/desktop-login/start',
      payload: {
        device_name: '',
        device_fingerprint: 'short',
        desktop_public_key: 'short',
        capabilities: {}
      }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(100101)
  })

  it('requires login for complete', async () => {
    const app = buildApp({
      registerDesktopLoginRoutes: async (instance) => {
        instance.post('/api/v1/desktop-login/complete', async () =>
          apiFailure(ErrorCode.UNAUTHORIZED, '未登录')
        )
      }
    })
    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/desktop-login/complete',
      payload: { request_id: 'dlr_123' }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(200001)
  })
})
