import { describe, expect, it } from 'vitest'
import { bindRelaySocket, forwardRelayFrame } from '../src/ws/relay-socket.js'
import { createHash } from '../src/shared/crypto.js'

describe('/ws/relay bind', () => {
  it('binds when connection token and user/device ownership match', async () => {
    const tokenHash = createHash('cnt_valid_token_with_enough_length_123456', 'pepper')
    const result = await bindRelaySocket({
      query: {
        connection_id: 'conn_1',
        connection_token: 'cnt_valid_token_with_enough_length_123456',
        side: 'client'
      },
      actor: { kind: 'client', userId: 'usr_1' },
      tokenPepper: 'pepper',
      state: {
        async get() {
          return {
            connection_id: 'conn_1',
            user_id: 'usr_1',
            device_id: 'dev_1',
            client_id: 'web_1',
            token_hash: tokenHash,
            status: 'signaling',
            created_at: '2026-06-28T00:00:00.000Z',
            expires_at: '2099-01-01T00:00:00.000Z'
          }
        }
      }
    })

    expect(result).toEqual({
      ok: true,
      binding: {
        connectionId: 'conn_1',
        side: 'client',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1'
      }
    })
  })

  it('rejects mismatched token', async () => {
    const result = await bindRelaySocket({
      query: {
        connection_id: 'conn_1',
        connection_token: 'cnt_wrong_token_with_enough_length_123456',
        side: 'client'
      },
      actor: { kind: 'client', userId: 'usr_1' },
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

    expect(result).toEqual({ ok: false, code: 220403, message: '连接权限不足' })
  })
})

describe('/ws/relay frame forwarding', () => {
  it('forwards ciphertext frame without inspecting payload', async () => {
    const forwarded: object[] = []
    const result = await forwardRelayFrame({
      raw: JSON.stringify({
        version: 1,
        type: 'relay.frame',
        id: 'msg_1',
        connection_id: 'conn_1',
        seq: 1,
        ciphertext: 'eyJlbmNyeXB0ZWQiOiJvcGFxdWUifQ=='
      }),
      binding: {
        connectionId: 'conn_1',
        side: 'client',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1'
      },
      registry: {
        acceptSeq() {
          return true
        },
        forward(_connectionId: string, _side: 'client' | 'device', message: object) {
          forwarded.push(message)
          return true
        }
      }
    })

    expect(result).toEqual({ ok: true })
    expect(forwarded[0]).toMatchObject({
      type: 'relay.frame',
      connection_id: 'conn_1',
      ciphertext: 'eyJlbmNyeXB0ZWQiOiJvcGFxdWUifQ=='
    })
  })

  it('rejects repeated sequence numbers', async () => {
    const result = await forwardRelayFrame({
      raw: JSON.stringify({
        version: 1,
        type: 'relay.frame',
        id: 'msg_1',
        connection_id: 'conn_1',
        seq: 1,
        ciphertext: 'abc'
      }),
      binding: {
        connectionId: 'conn_1',
        side: 'client',
        userId: 'usr_1',
        deviceId: 'dev_1',
        clientId: 'web_1'
      },
      registry: {
        acceptSeq() {
          return false
        },
        forward() {
          return true
        }
      }
    })

    expect(result).toEqual({ ok: false, code: 220403, message: 'relay 帧序号无效' })
  })
})
