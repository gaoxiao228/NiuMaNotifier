import { createHash, createRandomToken } from '../../shared/crypto.js'
import { ErrorCode, type ErrorCodeValue } from '../../shared/errors.js'
import { addDays, addSeconds, systemClock, type Clock } from '../../shared/time.js'
import { hashPassword, verifyPassword } from './password.service.js'
import { createAccessToken } from './token.service.js'

export type AuthUser = {
  id: string
  email: string
  passwordHash: string
  passwordAlgo: string
  role: 'admin' | 'user'
  status: 'active' | 'disabled'
  createdAt: Date
  updatedAt: Date
  passwordUpdatedAt: Date
}

export type RefreshTokenRecord = {
  id: string
  userId: string
  tokenHash: string
  clientId: string
  expiresAt: Date
  revokedAt: Date | null
  rotatedFromId: string | null
  createdAt: Date
}

export type AuthRepository = {
  findUserByEmail(email: string): Promise<AuthUser | null>
  findUserById(id: string): Promise<AuthUser | null>
  createUser(input: Omit<AuthUser, 'id'>): Promise<AuthUser>
  createRefreshToken(input: Omit<RefreshTokenRecord, 'id'>): Promise<RefreshTokenRecord>
  findRefreshTokenByHash(tokenHash: string): Promise<RefreshTokenRecord | null>
  revokeRefreshToken(id: string): Promise<void>
  revokeAllRefreshTokens(userId: string): Promise<void>
}

export type ServiceFailure = {
  ok: false
  code: ErrorCodeValue
  message: string
}

export type ServiceSuccess<T extends object> = {
  ok: true
  data: T
}

export type AuthServiceConfig = {
  registrationMode: 'open' | 'admin_invite' | 'disabled'
  tokenPepper: string
  jwtPrivateKey: string
  jwtPublicKey: string
  accessTokenTtlSeconds: number
  refreshTokenTtlDays: number
}

function publicUser(user: AuthUser) {
  return {
    id: user.id,
    email: user.email,
    role: user.role,
    status: user.status
  }
}

function failure(code: ErrorCodeValue, message: string): ServiceFailure {
  return { ok: false, code, message }
}

export function createAuthService(options: {
  repo: AuthRepository
  config: AuthServiceConfig
  clock?: Clock
}) {
  const clock = options.clock ?? systemClock
  const { repo, config } = options

  async function issueSession(user: AuthUser, clientId: string, rotatedFromId: string | null = null) {
    const now = clock.now()
    const refreshToken = createRandomToken('rft')
    const record = await repo.createRefreshToken({
      userId: user.id,
      tokenHash: createHash(refreshToken, config.tokenPepper),
      clientId,
      expiresAt: addDays(now, config.refreshTokenTtlDays),
      revokedAt: null,
      rotatedFromId,
      createdAt: now
    })
    const accessToken = await createAccessToken(
      {
        userId: user.id,
        sessionId: record.id,
        role: user.role
      },
      {
        privateKeyPem: config.jwtPrivateKey,
        ttlSeconds: config.accessTokenTtlSeconds
      }
    )

    return {
      access_token: accessToken,
      refresh_token: refreshToken,
      expires_at: addSeconds(now, config.accessTokenTtlSeconds).toISOString(),
      user: publicUser(user)
    }
  }

  return {
    async register(input: { email: string; password: string }) {
      if (config.registrationMode !== 'open') {
        return failure(ErrorCode.REGISTRATION_MODE_FORBIDDEN, '当前不允许开放注册')
      }

      const email = input.email.toLowerCase()
      const existing = await repo.findUserByEmail(email)
      if (existing) return failure(ErrorCode.EMAIL_ALREADY_REGISTERED, '邮箱已注册')

      const password = await hashPassword(input.password)
      const now = clock.now()
      const user = await repo.createUser({
        email,
        passwordHash: password.hash,
        passwordAlgo: password.algo,
        role: 'user',
        status: 'active',
        createdAt: now,
        updatedAt: now,
        passwordUpdatedAt: now
      })

      return { ok: true, data: { user: publicUser(user) } } satisfies ServiceSuccess<object>
    },

    async login(input: { email: string; password: string; clientId: string }) {
      const user = await repo.findUserByEmail(input.email.toLowerCase())
      if (!user) return failure(ErrorCode.ACCOUNT_NOT_FOUND, '账号不存在')
      if (user.status !== 'active') return failure(ErrorCode.ACCOUNT_DISABLED, '账号已禁用')

      const passwordValid = await verifyPassword(user.passwordHash, input.password)
      if (!passwordValid) return failure(ErrorCode.PASSWORD_INCORRECT, '密码错误')

      return {
        ok: true,
        data: await issueSession(user, input.clientId)
      } satisfies ServiceSuccess<object>
    },

    async refresh(input: { refreshToken: string; clientId: string }) {
      const tokenHash = createHash(input.refreshToken, config.tokenPepper)
      const token = await repo.findRefreshTokenByHash(tokenHash)
      if (!token || token.revokedAt) return failure(ErrorCode.TOKEN_INVALID, 'Token 无效')
      if (token.expiresAt.getTime() <= clock.now().getTime()) {
        return failure(ErrorCode.TOKEN_EXPIRED, 'Token 已过期')
      }

      const user = await repo.findUserById(token.userId)
      if (!user) return failure(ErrorCode.ACCOUNT_NOT_FOUND, '账号不存在')
      if (user.status !== 'active') return failure(ErrorCode.ACCOUNT_DISABLED, '账号已禁用')

      await repo.revokeRefreshToken(token.id)
      return {
        ok: true,
        data: await issueSession(user, input.clientId, token.id)
      } satisfies ServiceSuccess<object>
    },

    async logout(input: { refreshToken: string }) {
      const token = await repo.findRefreshTokenByHash(createHash(input.refreshToken, config.tokenPepper))
      if (token) await repo.revokeRefreshToken(token.id)
      return { ok: true, data: {} } satisfies ServiceSuccess<object>
    },

    async logoutAll(userId: string) {
      await repo.revokeAllRefreshTokens(userId)
      return { ok: true, data: {} } satisfies ServiceSuccess<object>
    },

    async me(userId: string) {
      const user = await repo.findUserById(userId)
      if (!user || user.status !== 'active') return failure(ErrorCode.UNAUTHORIZED, '未登录')

      return {
        ok: true,
        data: { user: publicUser(user) }
      } satisfies ServiceSuccess<object>
    }
  }
}
