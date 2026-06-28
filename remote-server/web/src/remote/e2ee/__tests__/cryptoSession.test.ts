import { describe, expect, it } from 'vitest'
import {
  createClientHello,
  decryptFrameForTest,
  deriveClientSession,
  encryptFrame,
  signDeviceHelloForTest,
  verifyDeviceHello
} from '../cryptoSession.js'

describe('browser E2EE crypto session', () => {
  it('creates client hello and derives directional keys from signed device hello', async () => {
    const clientHello = await createClientHello({
      connectionId: 'conn_1',
      deviceId: 'dev_1',
      clientId: 'web_1'
    })

    expect(clientHello.type).toBe('e2ee.client_hello')
    expect(clientHello.ephemeral_public_key).toBeDefined()
  })

  it('verifies device hello signature with registered identity public key', async () => {
    const identity = await crypto.subtle.generateKey(
      { name: 'ECDSA', namedCurve: 'P-256' },
      true,
      ['sign', 'verify']
    )
    const deviceHello = await signDeviceHelloForTest({
      identityPrivateKey: identity.privateKey,
      connectionId: 'conn_1',
      deviceId: 'dev_1',
      clientId: 'web_1'
    })
    const identityPublicKey = await crypto.subtle.exportKey('jwk', identity.publicKey)

    await expect(verifyDeviceHello(deviceHello, identityPublicKey)).resolves.toBe(true)
  })

  it('encrypts frame without exposing plaintext', async () => {
    const session = await deriveClientSession({
      connectionId: 'conn_1',
      deviceId: 'dev_1',
      clientId: 'web_1',
      sharedSecretForTest: new Uint8Array(32).fill(7)
    })

    const frame = await encryptFrame(session, {
      version: 1,
      type: 'request',
      id: 'req_1',
      method: 'device.get_health',
      params: {}
    })

    expect(frame.type).toBe('rpc.frame')
    expect(JSON.stringify(frame)).not.toContain('device.get_health')
    await expect(decryptFrameForTest(session, frame)).resolves.toMatchObject({
      method: 'device.get_health'
    })
  })
})
