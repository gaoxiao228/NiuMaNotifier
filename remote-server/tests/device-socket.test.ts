import { describe, expect, it, vi } from 'vitest'
import { createDeviceSocketRegistry } from '../src/modules/devices/device-socket-registry.js'
import { handleDeviceMessage } from '../src/ws/device-socket.js'
import { deviceSocketMessageSchema } from '../src/ws/ws-message.schemas.js'

describe('device websocket schema and registry', () => {
  it('accepts hello and heartbeat messages', () => {
    expect(
      deviceSocketMessageSchema.parse({
        version: 1,
        type: 'device.hello',
        id: 'msg_1',
        data: {
          device_id: 'dev_1',
          agent_protocol_version: 1,
          rpc_protocol_version: 1,
          capabilities: { supports_webrtc: true }
        }
      }).type
    ).toBe('device.hello')

    expect(
      deviceSocketMessageSchema.parse({
        version: 1,
        type: 'device.heartbeat',
        id: 'msg_2',
        data: {}
      }).type
    ).toBe('device.heartbeat')
  })

  it('accepts device connection and signal response messages', () => {
    expect(
      deviceSocketMessageSchema.parse({
        version: 1,
        type: 'connection.accept',
        id: 'msg_3',
        data: { connection_id: 'conn_1', transport: 'webrtc' }
      }).type
    ).toBe('connection.accept')

    expect(
      deviceSocketMessageSchema.parse({
        version: 1,
        type: 'signal.answer',
        id: 'msg_4',
        data: { connection_id: 'conn_1', sdp: 'answer-sdp' }
      }).type
    ).toBe('signal.answer')
  })

  it('rejects device response messages missing protocol required fields', () => {
    expect(() => deviceSocketMessageSchema.parse({
      version: 1,
      type: 'connection.accept',
      id: 'msg_3',
      data: { connection_id: 'conn_1' }
    })).toThrow()

    expect(() => deviceSocketMessageSchema.parse({
      version: 1,
      type: 'connection.reject',
      id: 'msg_4',
      data: { connection_id: 'conn_1' }
    })).toThrow()

    expect(() => deviceSocketMessageSchema.parse({
      version: 1,
      type: 'signal.answer',
      id: 'msg_5',
      data: { connection_id: 'conn_1' }
    })).toThrow()

    expect(() => deviceSocketMessageSchema.parse({
      version: 1,
      type: 'signal.ice_candidate',
      id: 'msg_6',
      data: { connection_id: 'conn_1' }
    })).toThrow()

    expect(() => deviceSocketMessageSchema.parse({
      version: 1,
      type: 'signal.cancel',
      id: 'msg_7',
      data: { connection_id: 'conn_1' }
    })).toThrow()
  })

  it('rejects overlong device response fields', () => {
    const longReason = 'x'.repeat(241)

    expect(() => deviceSocketMessageSchema.parse({
      version: 1,
      type: 'signal.cancel',
      id: 'msg_8',
      data: { connection_id: 'conn_1', reason: longReason }
    })).toThrow()
  })

  it('closes a registered socket when device is revoked', () => {
    const registry = createDeviceSocketRegistry()
    const close = vi.fn()

    registry.add('dev_1', { close })
    expect(registry.has('dev_1')).toBe(true)
    registry.closeDevice('dev_1', 4003, 'token_revoked')

    expect(close).toHaveBeenCalledWith(4003, 'token_revoked')
    expect(registry.has('dev_1')).toBe(false)
  })
})

describe('device websocket lifecycle', () => {
  it('marks device online on hello and heartbeat', async () => {
    const calls: string[] = []
    const service = {
      async markOnline(input: any) {
        calls.push(`${input.deviceId}:${input.socketId}`)
      },
      async markOffline() {}
    }
    const repo = {
      async updateLastSeen() {
        calls.push('last_seen')
      }
    }

    await handleDeviceMessage({
      raw: JSON.stringify({
        version: 1,
        type: 'device.hello',
        id: 'msg_1',
        data: {
          device_id: 'dev_1',
          agent_protocol_version: 1,
          rpc_protocol_version: 1,
          capabilities: { supports_webrtc: true }
        }
      }),
      authenticatedDevice: { id: 'dev_1', userId: 'usr_1' },
      socketId: 'sock_1',
      serverInstanceId: 'srv_1',
      presence: service,
      devices: repo
    })

    await handleDeviceMessage({
      raw: JSON.stringify({
        version: 1,
        type: 'device.heartbeat',
        id: 'msg_2',
        data: {}
      }),
      authenticatedDevice: { id: 'dev_1', userId: 'usr_1' },
      socketId: 'sock_1',
      serverInstanceId: 'srv_1',
      presence: service,
      devices: repo
    })

    expect(calls).toContain('dev_1:sock_1')
    expect(calls).toContain('last_seen')
  })

  it('returns forward_to_client for device connection and signal responses', async () => {
    const deps = {
      authenticatedDevice: { id: 'dev_1', userId: 'usr_1' },
      socketId: 'sock_1',
      serverInstanceId: 'srv_1',
      presence: {
        async markOnline() {}
      },
      devices: {
        async updateLastSeen() {}
      }
    }

    const accepted = await handleDeviceMessage({
      ...deps,
      raw: JSON.stringify({
        version: 1,
        type: 'connection.accept',
        id: 'msg_3',
        data: { connection_id: 'conn_1', transport: 'webrtc' }
      })
    })
    const answered = await handleDeviceMessage({
      ...deps,
      raw: JSON.stringify({
        version: 1,
        type: 'signal.answer',
        id: 'msg_4',
        data: { connection_id: 'conn_1', sdp: 'answer-sdp' }
      })
    })

    expect(accepted).toEqual({
      kind: 'forward_to_client',
      connectionId: 'conn_1',
      message: {
        version: 1,
        type: 'connection.accept',
        id: 'msg_3',
        data: { connection_id: 'conn_1', transport: 'webrtc' }
      }
    })
    expect(answered).toMatchObject({
      kind: 'forward_to_client',
      connectionId: 'conn_1',
      message: {
        type: 'signal.answer',
        data: { connection_id: 'conn_1', sdp: 'answer-sdp' }
      }
    })
  })
})
