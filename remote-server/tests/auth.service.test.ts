import { describe, expect, it } from 'vitest'
import { exportPKCS8, exportSPKI, generateKeyPair } from 'jose'
import { createAuthService, type AuthRepository } from '../src/modules/auth/auth.service.js'
import { hashPassword } from '../src/modules/auth/password.service.js'
import { ErrorCode } from '../src/shared/errors.js'

function createFakeRepo(): AuthRepository {
  const users = new Map<string, any>()
  const refreshTokens = new Map<string, any>()

  return {
    async findUserByEmail(email) {
      return [...users.values()].find((user) => user.email === email) ?? null
    },
    async findUserById(id) {
      return users.get(id) ?? null
    },
    async createUser(input) {
      const user = { id: `usr_${users.size + 1}`, ...input }
      users.set(user.id, user)
      return user
    },
    async createRefreshToken(input) {
      const token = { id: `rft_${refreshTokens.size + 1}`, ...input }
      refreshTokens.set(input.tokenHash, token)
      return token
    },
    async findRefreshTokenByHash(tokenHash) {
      return refreshTokens.get(tokenHash) ?? null
    },
    async revokeRefreshToken(id) {
      for (const token of refreshTokens.values()) {
        if (token.id === id) token.revokedAt = new Date('2026-06-28T00:00:00Z')
      }
    },
    async revokeAllRefreshTokens(userId) {
      for (const token of refreshTokens.values()) {
        if (token.userId === userId) token.revokedAt = new Date('2026-06-28T00:00:00Z')
      }
    }
  }
}

describe('auth service', () => {
  it('registers a user when registration is open', async () => {
    const service = createAuthService({
      repo: createFakeRepo(),
      config: {
        registrationMode: 'open',
        tokenPepper: 'pepper',
        jwtPrivateKey: 'test-private-key',
        jwtPublicKey: 'test-public-key',
        accessTokenTtlSeconds: 900,
        refreshTokenTtlDays: 30
      }
    })

    const result = await service.register({ email: 'user@example.com', password: 'password123' })

    expect(result.ok).toBe(true)
    if (result.ok) {
      expect(result.data.user.email).toBe('user@example.com')
      expect(result.data.user.role).toBe('user')
    }
  })

  it('rejects duplicate email as business failure', async () => {
    const repo = createFakeRepo()
    const service = createAuthService({
      repo,
      config: {
        registrationMode: 'open',
        tokenPepper: 'pepper',
        jwtPrivateKey: 'test-private-key',
        jwtPublicKey: 'test-public-key',
        accessTokenTtlSeconds: 900,
        refreshTokenTtlDays: 30
      }
    })

    await service.register({ email: 'user@example.com', password: 'password123' })
    const result = await service.register({ email: 'user@example.com', password: 'password123' })

    expect(result).toEqual({
      ok: false,
      code: ErrorCode.EMAIL_ALREADY_REGISTERED,
      message: '邮箱已注册'
    })
  })

  it('logs in and rotates refresh token', async () => {
    const { privateKey, publicKey } = await generateKeyPair('EdDSA')
    const privateKeyPem = await exportPKCS8(privateKey)
    const publicKeyPem = await exportSPKI(publicKey)
    const repo = createFakeRepo()
    const password = await hashPassword('password123')
    await repo.createUser({
      email: 'user@example.com',
      passwordHash: password.hash,
      passwordAlgo: password.algo,
      role: 'user',
      status: 'active',
      createdAt: new Date(),
      updatedAt: new Date(),
      passwordUpdatedAt: new Date()
    })

    const service = createAuthService({
      repo,
      config: {
        registrationMode: 'open',
        tokenPepper: 'pepper',
        jwtPrivateKey: privateKeyPem,
        jwtPublicKey: publicKeyPem,
        accessTokenTtlSeconds: 900,
        refreshTokenTtlDays: 30
      }
    })

    const login = await service.login({
      email: 'user@example.com',
      password: 'password123',
      clientId: 'web'
    })
    expect(login.ok).toBe(true)
    if (!login.ok) throw new Error('login failed')

    const refresh = await service.refresh({ refreshToken: login.data.refresh_token, clientId: 'web' })
    expect(refresh.ok).toBe(true)
    if (refresh.ok) expect(refresh.data.refresh_token).not.toBe(login.data.refresh_token)
  })

  it('rejects non-admin users from admin login', async () => {
    const { privateKey, publicKey } = await generateKeyPair('EdDSA')
    const privateKeyPem = await exportPKCS8(privateKey)
    const publicKeyPem = await exportSPKI(publicKey)
    const repo = createFakeRepo()
    const password = await hashPassword('password123')
    await repo.createUser({
      email: 'user@example.com',
      passwordHash: password.hash,
      passwordAlgo: password.algo,
      role: 'user',
      status: 'active',
      createdAt: new Date(),
      updatedAt: new Date(),
      passwordUpdatedAt: new Date()
    })

    const service = createAuthService({
      repo,
      config: {
        registrationMode: 'open',
        tokenPepper: 'pepper',
        jwtPrivateKey: privateKeyPem,
        jwtPublicKey: publicKeyPem,
        accessTokenTtlSeconds: 900,
        refreshTokenTtlDays: 30
      }
    })

    const result = await service.loginAdmin({
      email: 'user@example.com',
      password: 'password123',
      clientId: 'admin-web'
    })

    expect(result).toEqual({
      ok: false,
      code: ErrorCode.ADMIN_FORBIDDEN,
      message: '需要管理员权限'
    })
  })

  it('allows admin users to use admin login', async () => {
    const { privateKey, publicKey } = await generateKeyPair('EdDSA')
    const privateKeyPem = await exportPKCS8(privateKey)
    const publicKeyPem = await exportSPKI(publicKey)
    const repo = createFakeRepo()
    const password = await hashPassword('password123')
    await repo.createUser({
      email: 'admin@example.com',
      passwordHash: password.hash,
      passwordAlgo: password.algo,
      role: 'admin',
      status: 'active',
      createdAt: new Date(),
      updatedAt: new Date(),
      passwordUpdatedAt: new Date()
    })

    const service = createAuthService({
      repo,
      config: {
        registrationMode: 'open',
        tokenPepper: 'pepper',
        jwtPrivateKey: privateKeyPem,
        jwtPublicKey: publicKeyPem,
        accessTokenTtlSeconds: 900,
        refreshTokenTtlDays: 30
      }
    })

    const result = await service.loginAdmin({
      email: 'admin@example.com',
      password: 'password123',
      clientId: 'admin-web'
    })

    expect(result.ok).toBe(true)
    if (result.ok) expect(result.data.user.role).toBe('admin')
  })
})
