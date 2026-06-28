import { describe, expect, it } from 'vitest'
import { hashPassword, verifyPassword } from '../src/modules/auth/password.service.js'

describe('password service', () => {
  it('hashes and verifies passwords using argon2id', async () => {
    const result = await hashPassword('correct horse battery staple')

    expect(result.algo).toBe('argon2id')
    expect(result.hash).not.toContain('correct horse')
    await expect(verifyPassword(result.hash, 'correct horse battery staple')).resolves.toBe(true)
    await expect(verifyPassword(result.hash, 'wrong password')).resolves.toBe(false)
  })
})
