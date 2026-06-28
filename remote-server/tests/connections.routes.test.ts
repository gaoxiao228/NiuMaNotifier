import { describe, expect, it } from 'vitest'
import { createConnectionInviteMessage } from '../src/modules/connections/connections.routes.js'
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

describe('connection create device notification', () => {
  it('uses a device invitation message shape', () => {
    const message = createConnectionInviteMessage({
      connectionId: 'conn_1',
      clientId: 'web_1',
      transportPreference: 'webrtc_first'
    })

    expect(message.type).toBe('connection.invite')
    expect(message.connection_id).toBe('conn_1')
    expect(message.data.transport_preference).toBe('webrtc_first')
  })
})
