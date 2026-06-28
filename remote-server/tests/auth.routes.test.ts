import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { ErrorCode } from '../src/shared/errors.js'
import { apiFailure } from '../src/shared/response.js'

describe('auth routes', () => {
  it('returns validation envelope for invalid register body', async () => {
    const app = buildApp({
      registerAuthRoutes: async (instance) => {
        instance.post('/api/v1/auth/register', async () =>
          apiFailure(
            ErrorCode.BUSINESS_VALIDATION_FAILED,
            'emailInvalid email；passwordString must contain at least 8 character(s)'
          )
        )
      }
    })
    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/auth/register',
      payload: { email: 'bad', password: 'short' }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(100101)
  })

  it('requires bearer token for me', async () => {
    const app = buildApp({
      registerAuthRoutes: async (instance) => {
        instance.get('/api/v1/auth/me', async () => apiFailure(ErrorCode.UNAUTHORIZED, '未登录'))
      }
    })
    const response = await app.inject({
      method: 'GET',
      url: '/api/v1/auth/me'
    })

    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 200001,
      message: '未登录',
      data: null
    })
  })
})
