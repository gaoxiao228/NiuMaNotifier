import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { ErrorCode } from '../src/shared/errors.js'
import { apiFailure } from '../src/shared/response.js'

describe('desktop login routes', () => {
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
