import { describe, expect, it, vi } from 'vitest'
import { clientSignalMessageSchema } from '../src/modules/connections/connections.schemas.js'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'
import { bindClientConnection, forwardClientSignal } from '../src/ws/client-socket.js'

describe('client signaling prerequisites', () => {
  it('sends messages to registered device socket', () => {
    const registry = createDeviceSocketRegistry()
    const send = vi.fn()
    registry.add('dev_1', { close: vi.fn(), send })

    expect(registry.sendToDevice('dev_1', { type: 'signal.offer' })).toBe(true)
    expect(send).toHaveBeenCalledWith(JSON.stringify({ type: 'signal.offer' }))
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
})

describe('/ws/client signaling', () => {
  it('binds only when bearer user and connection token match Redis state', async () => {
    const result = await bindClientConnection({
      auth: { userId: 'usr_1', sessionId: 'rft_1', role: 'user' },
      query: { connection_id: 'conn_1', connection_token: 'cnt_token' },
      tokenPepper: 'pepper',
      state: {
        async get() {
          return {
            connection_id: 'conn_1',
            user_id: 'usr_1',
            device_id: 'dev_1',
            client_id: 'web_1',
            token_hash: 'bad_hash',
            status: 'signaling',
            created_at: '2026-06-28T00:00:00.000Z',
            expires_at: '2099-01-01T00:00:00.000Z'
          }
        }
      }
    })

    expect(result.ok).toBe(false)
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
      connection_id: 'conn_1'
    })
  })
})
