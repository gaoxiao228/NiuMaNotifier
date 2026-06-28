import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { ErrorCode } from '../src/shared/errors.js'
import { apiFailure, apiSuccess } from '../src/shared/response.js'

describe('connection routes', () => {
  it('requires bearer token for create', async () => {
    const app = buildApp({
      registerConnectionsRoutes: async (instance) => {
        instance.post('/api/v1/connections/create', async () =>
          apiFailure(ErrorCode.UNAUTHORIZED, '未登录')
        )
      }
    })

    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/connections/create',
      payload: { device_id: 'dev_1', client_id: 'web_1', transport_preference: 'webrtc_first' }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(200001)
  })

  it('returns ice config envelope', async () => {
    const app = buildApp({
      registerConnectionsRoutes: async (instance) => {
        instance.get('/api/v1/connections/ice-config', async () => apiSuccess({ ice_servers: [] }))
      }
    })

    const response = await app.inject({
      method: 'GET',
      url: '/api/v1/connections/ice-config'
    })

    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 0,
      message: 'ok',
      data: { ice_servers: [] }
    })
  })
})
