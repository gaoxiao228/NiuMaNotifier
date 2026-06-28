import { describe, expect, it } from 'vitest'
import { exportPKCS8, exportSPKI, generateKeyPair } from 'jose'
import { createAccessToken, verifyAccessToken } from '../src/modules/auth/token.service.js'
import { createHash, createRandomToken } from '../src/shared/crypto.js'
import { createPublicId } from '../src/shared/id.js'

describe('shared token helpers', () => {
  it('creates high entropy tokens and stores only peppered hashes', () => {
    const first = createRandomToken('rt')
    const second = createRandomToken('rt')

    expect(first).toMatch(/^rt_[A-Za-z0-9_-]{43,}$/)
    expect(second).not.toBe(first)
    expect(createHash(first, 'pepper')).toHaveLength(64)
    expect(createHash(first, 'pepper')).not.toBe(first)
  })

  it('creates prefixed public ids', () => {
    expect(createPublicId('usr')).toMatch(/^usr_[A-Za-z0-9_-]{21,}$/)
    expect(createPublicId('dlr')).toMatch(/^dlr_[A-Za-z0-9_-]{21,}$/)
  })
})

describe('access token service', () => {
  it('creates and verifies JWT access tokens', async () => {
    const { privateKey, publicKey } = await generateKeyPair('EdDSA')
    const privateKeyPem = await exportPKCS8(privateKey)
    const publicKeyPem = await exportSPKI(publicKey)
    const token = await createAccessToken(
      {
        userId: 'usr_123',
        sessionId: 'rt_123',
        role: 'user'
      },
      {
        privateKeyPem,
        ttlSeconds: 900
      }
    )

    const payload = await verifyAccessToken(token, publicKeyPem)
    expect(payload.userId).toBe('usr_123')
    expect(payload.sessionId).toBe('rt_123')
    expect(payload.role).toBe('user')
  })
})
