import { describe, expect, it } from 'vitest'
import {
  desktopLoginPollSchema,
  desktopLoginStartSchema
} from '../src/modules/desktopLogin/desktopLogin.schemas.js'

describe('desktop login schemas', () => {
  it('accepts valid start request', () => {
    const result = desktopLoginStartSchema.parse({
      device_name: 'NiuMa MacBook',
      device_fingerprint: 'a'.repeat(64),
      desktop_public_key: 'base64-public-key',
      device_identity_public_key: JSON.stringify({
        kty: 'EC',
        crv: 'P-256',
        x: 'x-coordinate',
        y: 'y-coordinate'
      }),
      capabilities: {
        agent_protocol_version: 1,
        rpc_protocol_version: 1,
        supports_webrtc: true,
        supports_relay: true,
        supports_remote_control: true
      }
    })

    expect(result.device_name).toBe('NiuMa MacBook')
  })

  it('requires poll token for polling', () => {
    expect(() =>
      desktopLoginPollSchema.parse({
        request_id: 'dlr_123'
      })
    ).toThrow()
  })
})

describe('desktop login identity key schema', () => {
  it('requires device identity public key', () => {
    const result = desktopLoginStartSchema.parse({
      device_name: 'NiuMa MacBook',
      device_fingerprint: 'a'.repeat(64),
      desktop_public_key: 'base64-public-key',
      device_identity_public_key: JSON.stringify({
        kty: 'EC',
        crv: 'P-256',
        x: 'x-coordinate',
        y: 'y-coordinate'
      }),
      capabilities: {
        agent_protocol_version: 1,
        rpc_protocol_version: 1,
        supports_webrtc: true,
        supports_relay: true,
        supports_remote_control: true
      }
    })

    expect(result.device_identity_public_key).toContain('P-256')
  })
})
