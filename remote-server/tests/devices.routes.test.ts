import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { ErrorCode } from '../src/shared/errors.js'
import { apiFailure } from '../src/shared/response.js'

describe('devices routes', () => {
  it('requires auth for device list', async () => {
    const app = buildApp({
      registerDevicesRoutes: async (instance) => {
        instance.get('/api/v1/devices/list', async () => apiFailure(ErrorCode.UNAUTHORIZED, '未登录'))
      }
    })
    const response = await app.inject({
      method: 'GET',
      url: '/api/v1/devices/list'
    })

    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 200001,
      message: '未登录',
      data: null
    })
  })
})
