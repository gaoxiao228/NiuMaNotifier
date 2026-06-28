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
  it('uses a RemoteAgent compatible invitation message shape', () => {
    const message = createConnectionInviteMessage({
      connectionId: 'conn_1',
      clientId: 'web_1',
      transportPreference: 'webrtc_first',
      expiresAt: '2026-06-28T00:02:00.000Z'
    })

    expect(message).toEqual({
      version: 1,
      type: 'connection.invite',
      id: 'msg_conn_1',
      data: {
        connection_id: 'conn_1',
        client_id: 'web_1',
        transport_preference: 'auto',
        expires_at: '2026-06-28T00:02:00.000Z'
      }
    })
    expect(message).not.toHaveProperty('connection_id')
  })

  it('maps server transport preferences to RemoteAgent transport preferences', () => {
    expect(createConnectionInviteMessage({
      connectionId: 'conn_1',
      clientId: 'web_1',
      transportPreference: 'webrtc_first',
      expiresAt: '2026-06-28T00:02:00.000Z'
    }).data.transport_preference).toBe('auto')
    expect(createConnectionInviteMessage({
      connectionId: 'conn_2',
      clientId: 'web_1',
      transportPreference: 'relay_first',
      expiresAt: '2026-06-28T00:02:00.000Z'
    }).data.transport_preference).toBe('relay')
    expect(createConnectionInviteMessage({
      connectionId: 'conn_3',
      clientId: 'web_1',
      transportPreference: 'relay_only',
      expiresAt: '2026-06-28T00:02:00.000Z'
    }).data.transport_preference).toBe('relay')
  })
})
