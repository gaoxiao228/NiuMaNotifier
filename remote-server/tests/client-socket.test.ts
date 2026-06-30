import { describe, expect, it, vi } from 'vitest'
import { createConnectionTokenService } from '../src/modules/connections/connection-token.service.js'
import { clientSignalMessageSchema } from '../src/modules/connections/connections.schemas.js'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'
import {
  bindClientConnection,
  createClientCancelMessage,
  forwardClientSignal,
  inviteDeviceForBoundClient
} from '../src/ws/client-socket.js'

describe('client signaling prerequisites', () => {
  it('sends messages to registered device socket', () => {
    const registry = createDeviceSocketRegistry()
    const send = vi.fn()
    registry.add('dev_1', { close: vi.fn(), send })

    expect(registry.sendToDevice('dev_1', { type: 'signal.offer' })).toBe(true)
    expect(send).toHaveBeenCalledWith(JSON.stringify({ type: 'signal.offer' }))
  })

  it('binds client sockets and sends messages to bound client socket', () => {
    const registry = createDeviceSocketRegistry()
    const send = vi.fn()

    registry.bindClient('conn_1', { close: vi.fn(), send })

    expect(registry.sendToClient('conn_1', { type: 'connection.accept' })).toBe(true)
    expect(send).toHaveBeenCalledWith(JSON.stringify({ type: 'connection.accept' }))
  })

  it('does not let a stale client close remove a newer client binding', () => {
    const registry = createDeviceSocketRegistry()
    const oldSocket = { close: vi.fn(), send: vi.fn() }
    const newSocket = { close: vi.fn(), send: vi.fn() }

    registry.bindClient('conn_1', oldSocket)
    registry.bindClient('conn_1', newSocket)
    registry.unbindClient('conn_1', oldSocket)

    expect(registry.sendToClient('conn_1', { type: 'connection.accept' })).toBe(true)
    expect(oldSocket.send).not.toHaveBeenCalled()
    expect(newSocket.send).toHaveBeenCalledWith(JSON.stringify({ type: 'connection.accept' }))
  })

  it('returns false when sending to client socket fails', () => {
    const registry = createDeviceSocketRegistry()

    registry.bindClient('conn_1', {
      close: vi.fn(),
      send() {
        throw new Error('send_failed')
      }
    })

    expect(registry.sendToClient('conn_1', { type: 'connection.accept' })).toBe(false)
  })

  it('validates signaling messages', () => {
    expect(
      clientSignalMessageSchema.parse({
        version: 1,
        id: 'msg_1',
        type: 'signal.offer',
        data: { sdp: 'offer-sdp' }
      }).type
    ).toBe('signal.offer')
  })

  it('accepts browser ice candidates with empty sdp mid', () => {
    expect(
      clientSignalMessageSchema.parse({
        version: 1,
        id: 'msg_ice',
        type: 'signal.ice_candidate',
        data: {
          candidate: 'candidate:1 1 udp 2122260223 192.0.2.1 54400 typ host',
          sdp_mid: '',
          sdp_mline_index: 0
        }
      }).type
    ).toBe('signal.ice_candidate')
  })

  it('rejects client signal messages missing protocol required fields', () => {
    expect(() => clientSignalMessageSchema.parse({
      version: 1,
      id: 'msg_1',
      type: 'signal.offer',
      data: {}
    })).toThrow()

    expect(() => clientSignalMessageSchema.parse({
      version: 1,
      id: 'msg_2',
      type: 'signal.ice_candidate',
      data: {}
    })).toThrow()
  })

  it('rejects answer messages from web client role', () => {
    expect(() => clientSignalMessageSchema.parse({
      version: 1,
      id: 'msg_3',
      type: 'signal.answer',
      data: { sdp: 'answer-sdp' }
    })).toThrow()
  })
})

describe('/ws/client signaling', () => {
  it('binds with connection token and Redis state without bearer auth', async () => {
    const tokenService = createConnectionTokenService({ tokenPepper: 'pepper' })
    const issued = tokenService.issue()

    const result = await bindClientConnection({
      query: { connection_id: 'conn_1', connection_token: issued.token },
      tokenPepper: 'pepper',
      state: {
        async get() {
          return {
            connection_id: 'conn_1',
            user_id: 'usr_1',
            device_id: 'dev_1',
            client_id: 'web_1',
            token_hash: issued.tokenHash,
            transport_preference: 'relay_first',
            status: 'signaling',
            created_at: '2026-06-28T00:00:00.000Z',
            expires_at: '2099-01-01T00:00:00.000Z'
          }
        }
      }
    })

    expect(result).toEqual({
      ok: true,
      connection: {
        connectionId: 'conn_1',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1',
        connectionToken: issued.token,
        transportPreference: 'relay_first',
        expiresAt: '2099-01-01T00:00:00.000Z'
      }
    })
  })

  it('sends device invite after client websocket is bound', () => {
    const sent: object[] = []
    const invited = inviteDeviceForBoundClient({
      connection: {
        connectionId: 'conn_1',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1',
        connectionToken: 'cnt_relay_secret',
        transportPreference: 'relay_first',
        expiresAt: '2026-06-28T00:02:00.000Z'
      },
      registry: {
        sendToDevice(deviceId: string, message: object) {
          sent.push({ deviceId, message })
          return true
        }
      }
    })

    expect(invited).toBe(true)
    expect(sent).toEqual([
      {
        deviceId: 'dev_1',
        message: {
          version: 1,
          type: 'connection.invite',
          id: 'msg_conn_1',
          data: {
            connection_id: 'conn_1',
            connection_token: 'cnt_relay_secret',
            client_id: 'web_1',
            transport_preference: 'relay',
            expires_at: '2026-06-28T00:02:00.000Z'
          }
        }
      }
    ])
  })

  it('builds a cancel message for device session cleanup when client websocket closes', () => {
    expect(createClientCancelMessage('conn_1')).toMatchObject({
      version: 1,
      type: 'signal.cancel',
      data: {
        connection_id: 'conn_1',
        reason: 'client_closed'
      }
    })
  })

  it('rejects missing, expired, and mismatched connection tokens', async () => {
    const tokenService = createConnectionTokenService({ tokenPepper: 'pepper' })
    const issued = tokenService.issue()
    const baseState = {
      connection_id: 'conn_1',
      user_id: 'usr_1',
      device_id: 'dev_1',
      client_id: 'web_1',
      token_hash: issued.tokenHash,
      transport_preference: 'relay_first',
      status: 'signaling' as const,
      created_at: '2026-06-28T00:00:00.000Z',
      expires_at: '2099-01-01T00:00:00.000Z'
    }

    const missing = await bindClientConnection({
      query: { connection_id: 'missing', connection_token: issued.token },
      tokenPepper: 'pepper',
      state: { async get() { return null } }
    })
    const expired = await bindClientConnection({
      query: { connection_id: 'conn_1', connection_token: issued.token },
      tokenPepper: 'pepper',
      state: {
        async get() {
          return { ...baseState, expires_at: '2000-01-01T00:00:00.000Z' }
        }
      }
    })
    const mismatched = await bindClientConnection({
      query: { connection_id: 'conn_1', connection_token: `${issued.token}_wrong` },
      tokenPepper: 'pepper',
      state: { async get() { return baseState } }
    })

    expect(missing).toMatchObject({ ok: false, code: 220401 })
    expect(expired).toMatchObject({ ok: false, code: 220402 })
    expect(mismatched).toMatchObject({ ok: false, code: 220403 })
  })

  it('forwards valid signal messages to device socket', async () => {
    const sent: object[] = []
    const result = await forwardClientSignal({
      raw: JSON.stringify({ version: 1, id: 'msg_1', type: 'signal.offer', data: { sdp: 'offer' } }),
      connection: { connectionId: 'conn_1', userId: 'usr_1', deviceId: 'dev_1', clientId: 'web_1' },
      registry: {
        sendToDevice(_deviceId: string, message: object) {
          sent.push(message)
          return true
        }
      }
    })

    expect(result).toEqual({ ok: true })
    expect(sent[0]).toMatchObject({
      version: 1,
      type: 'signal.offer',
      data: {
        sdp: 'offer',
        connection_id: 'conn_1',
        client_id: 'web_1'
      }
    })
    expect(sent[0]).not.toHaveProperty('connection_id')
    expect(sent[0]).not.toHaveProperty('client_id')
  })
})
