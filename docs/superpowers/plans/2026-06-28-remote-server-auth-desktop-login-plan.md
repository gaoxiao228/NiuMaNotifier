# Remote Server Auth And Desktop Login Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first usable account and desktop binding loop for the remote server: email/password auth, access/refresh token rotation, browser-driven desktop device binding, and authenticated device listing.

**Architecture:** This plan builds on `docs/superpowers/plans/2026-06-28-remote-server-foundation-plan.md`. It keeps the server as a modular Fastify monolith: routes parse HTTP, services own business rules, repositories own Drizzle access, and token/password helpers stay in shared modules. It intentionally does not implement `/ws/device`, WebRTC signaling, relay, or the Web console UI.

**Tech Stack:** Node.js LTS, TypeScript, Fastify, Zod, Drizzle ORM, PostgreSQL, Argon2id, jose JWT, Vitest.

---

## Scope Check

This plan covers:

- `POST /api/v1/auth/register`
- `POST /api/v1/auth/login`
- `POST /api/v1/auth/refresh`
- `POST /api/v1/auth/logout`
- `POST /api/v1/auth/logout-all`
- `GET /api/v1/auth/me`
- `POST /api/v1/desktop-login/start`
- `POST /api/v1/desktop-login/complete`
- `POST /api/v1/desktop-login/poll`
- `GET /api/v1/devices/list`

This plan does not cover:

- Admin user management UI.
- Email verification, forgot password, OAuth, Magic Link, or MFA.
- Device rename/unbind/revoke-token routes.
- `/ws/device`, Redis presence, WebRTC signaling, relay, or E2EE RPC transport.
- Real browser pages for `GET /desktop-login?request_id=...`; this plan only implements the JSON APIs the page will call.

## API Standard Notes

Follow the user-defined backend API standard:

- Business success and business failure return HTTP `200`.
- `code = 0` is the only success code.
- All JSON responses use `{ code, message, data }`.
- Auth failures such as invalid password, expired token, or disabled account are business failures and return HTTP `200 + non-zero code`.
- Protocol failures such as malformed JSON may return HTTP `400`, still with the same envelope.
- Business parameters use POST body or GET query; no dynamic path params.

## File Structure

Create:

- `remote-server/src/shared/time.ts` - injectable clock helpers.
- `remote-server/src/shared/id.ts` - prefixed ID helper for public IDs.
- `remote-server/src/modules/auth/auth.repository.ts` - Drizzle auth persistence.
- `remote-server/src/modules/auth/auth.service.ts` - register/login/refresh/logout business rules.
- `remote-server/src/modules/auth/auth.routes.ts` - auth HTTP routes.
- `remote-server/src/modules/auth/auth.middleware.ts` - Bearer access token parsing.
- `remote-server/src/modules/auth/password.service.ts` - Argon2id hash/verify wrapper.
- `remote-server/src/modules/auth/token.service.ts` - JWT and opaque refresh token handling.
- `remote-server/src/modules/desktopLogin/desktopLogin.repository.ts` - desktop login persistence.
- `remote-server/src/modules/desktopLogin/desktopLogin.crypto.ts` - encrypt desktop binding results to the one-time desktop public key.
- `remote-server/src/modules/desktopLogin/desktopLogin.service.ts` - start/complete/poll business rules.
- `remote-server/src/modules/desktopLogin/desktopLogin.routes.ts` - desktop-login HTTP routes.
- `remote-server/src/modules/devices/devices.repository.ts` - device persistence.
- `remote-server/src/modules/devices/devices.service.ts` - device listing and upsert helpers.
- `remote-server/src/modules/devices/devices.routes.ts` - device HTTP routes.
- `remote-server/tests/auth.service.test.ts`
- `remote-server/tests/auth.routes.test.ts`
- `remote-server/tests/token.service.test.ts`
- `remote-server/tests/password.service.test.ts`
- `remote-server/tests/desktopLogin.service.test.ts`
- `remote-server/tests/desktopLogin.routes.test.ts`
- `remote-server/tests/devices.routes.test.ts`

Modify:

- `remote-server/src/shared/errors.ts` - add admin and desktop-login error codes missing from foundation.
- `remote-server/src/shared/crypto.ts` - implement token generation and peppered token hashing.
- `remote-server/src/db/schema.ts` - add `audit_events` table and exported row types.
- `remote-server/src/app.ts` - register auth, desktop-login, and devices routes.
- `remote-server/src/modules/auth/auth.schemas.ts` - implement auth request schemas.
- `remote-server/src/modules/devices/devices.schemas.ts` - implement device schemas if foundation left it empty.
- `remote-server/src/modules/desktopLogin/desktopLogin.schemas.ts` - add response-facing status types if needed.

## Task 1: Shared Error, Time, ID, And Crypto Helpers

**Files:**
- Modify: `remote-server/src/shared/errors.ts`
- Modify: `remote-server/src/shared/crypto.ts`
- Create: `remote-server/src/shared/time.ts`
- Create: `remote-server/src/shared/id.ts`
- Test: `remote-server/tests/token.service.test.ts`

- [ ] **Step 1: Write failing shared helper tests**

Create `remote-server/tests/token.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
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
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- token.service.test.ts
```

Expected: FAIL because `createHash`, `createRandomToken`, or `createPublicId` are not implemented.

- [ ] **Step 3: Implement shared helpers**

Update `remote-server/src/shared/crypto.ts`:

```ts
import { createHash as nodeCreateHash, randomBytes } from 'node:crypto'

export function createRandomToken(prefix: string): string {
  return `${prefix}_${randomBytes(32).toString('base64url')}`
}

export function createHash(value: string, pepper: string): string {
  return nodeCreateHash('sha256').update(`${pepper}:${value}`).digest('hex')
}
```

Create `remote-server/src/shared/id.ts`:

```ts
import { randomBytes } from 'node:crypto'

export function createPublicId(prefix: string): string {
  return `${prefix}_${randomBytes(16).toString('base64url')}`
}
```

Create `remote-server/src/shared/time.ts`:

```ts
export type Clock = {
  now(): Date
}

export const systemClock: Clock = {
  now: () => new Date()
}

export function addSeconds(date: Date, seconds: number): Date {
  return new Date(date.getTime() + seconds * 1000)
}

export function addDays(date: Date, days: number): Date {
  return new Date(date.getTime() + days * 24 * 60 * 60 * 1000)
}

export function secondsUntil(now: Date, expiresAt: Date): number {
  return Math.max(0, Math.ceil((expiresAt.getTime() - now.getTime()) / 1000))
}
```

Update `remote-server/src/shared/errors.ts` so it includes these additional codes:

```ts
  ADMIN_FORBIDDEN: 230401,
  REGISTRATION_MODE_FORBIDDEN: 230402,
  BOOTSTRAP_ADMIN_EXISTS: 230501,
```

- [ ] **Step 4: Run shared helper tests**

Run:

```bash
cd remote-server
npm test -- token.service.test.ts
npm run build
```

Expected: both commands PASS.

- [ ] **Step 5: Commit**

```bash
git add remote-server/src/shared/errors.ts remote-server/src/shared/crypto.ts remote-server/src/shared/id.ts remote-server/src/shared/time.ts remote-server/tests/token.service.test.ts
git commit -m "feat: 新增远程服务端令牌基础工具" -m "修改内容：新增随机 token、加 pepper 哈希、公共 ID 和时间辅助函数，补齐管理错误码。" -m "修改原因：账号登录、设备绑定和 refresh token 轮换需要统一的安全令牌基础能力。"
```

## Task 2: Password And Access Token Services

**Files:**
- Create: `remote-server/src/modules/auth/password.service.ts`
- Create: `remote-server/src/modules/auth/token.service.ts`
- Test: `remote-server/tests/password.service.test.ts`
- Modify: `remote-server/tests/token.service.test.ts`

- [ ] **Step 1: Write failing password tests**

Create `remote-server/tests/password.service.test.ts`:

```ts
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
```

- [ ] **Step 2: Extend token tests**

Append to `remote-server/tests/token.service.test.ts`:

```ts
import { createAccessToken, verifyAccessToken } from '../src/modules/auth/token.service.js'
import { exportPKCS8, exportSPKI, generateKeyPair } from 'jose'

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
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cd remote-server
npm test -- password.service.test.ts token.service.test.ts
```

Expected: FAIL because auth password and token services do not exist.

- [ ] **Step 4: Implement password service**

Create `remote-server/src/modules/auth/password.service.ts`:

```ts
import { hash, verify } from '@node-rs/argon2'

export type PasswordHashResult = {
  hash: string
  algo: 'argon2id'
}

export async function hashPassword(password: string): Promise<PasswordHashResult> {
  return {
    hash: await hash(password, {
      memoryCost: 19456,
      timeCost: 2,
      parallelism: 1
    }),
    algo: 'argon2id'
  }
}

export async function verifyPassword(passwordHash: string, password: string): Promise<boolean> {
  return verify(passwordHash, password)
}
```

- [ ] **Step 5: Implement access token service**

Create `remote-server/src/modules/auth/token.service.ts`:

```ts
import { importPKCS8, importSPKI, SignJWT, jwtVerify } from 'jose'

export type AccessTokenInput = {
  userId: string
  sessionId: string
  role: 'admin' | 'user'
}

export type AccessTokenConfig = {
  privateKeyPem: string
  ttlSeconds: number
}

export async function createAccessToken(input: AccessTokenInput, config: AccessTokenConfig): Promise<string> {
  const privateKey = await importPKCS8(config.privateKeyPem, 'EdDSA')
  return new SignJWT({
    session_id: input.sessionId,
    role: input.role
  })
    .setProtectedHeader({ alg: 'EdDSA' })
    .setSubject(input.userId)
    .setIssuedAt()
    .setExpirationTime(`${config.ttlSeconds}s`)
    .sign(privateKey)
}

export async function verifyAccessToken(token: string, publicKeyPem: string) {
  const publicKey = await importSPKI(publicKeyPem, 'EdDSA')
  const { payload } = await jwtVerify(token, publicKey)

  return {
    userId: String(payload.sub),
    sessionId: String(payload.session_id),
    role: payload.role === 'admin' ? 'admin' : 'user'
  }
}
```

Access tokens use an asymmetric EdDSA key pair from `JWT_PRIVATE_KEY` and `JWT_PUBLIC_KEY`, matching the service design. The Docker/self-hosting docs must explain how to generate these PEM values before running the server.

- [ ] **Step 6: Run tests and build**

Run:

```bash
cd remote-server
npm test -- password.service.test.ts token.service.test.ts
npm run build
```

Expected: all PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/auth/password.service.ts remote-server/src/modules/auth/token.service.ts remote-server/tests/password.service.test.ts remote-server/tests/token.service.test.ts
git commit -m "feat: 新增远程服务端密码和访问令牌服务" -m "修改内容：新增 Argon2id 密码哈希校验和 JWT access token 签发校验。" -m "修改原因：邮箱密码登录和用户接口鉴权需要可测试的基础安全服务。"
```

## Task 3: Auth Schemas And Service Business Rules

**Files:**
- Modify: `remote-server/src/modules/auth/auth.schemas.ts`
- Create: `remote-server/src/modules/auth/auth.service.ts`
- Create: `remote-server/src/modules/auth/auth.repository.ts`
- Test: `remote-server/tests/auth.service.test.ts`

- [ ] **Step 1: Write failing auth service tests**

Create `remote-server/tests/auth.service.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { exportPKCS8, exportSPKI, generateKeyPair } from 'jose'
import { ErrorCode } from '../src/shared/errors.js'
import { createAuthService, type AuthRepository } from '../src/modules/auth/auth.service.js'
import { hashPassword } from '../src/modules/auth/password.service.js'

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
      refreshTokens.set(input.tokenHash, { id: `rft_${refreshTokens.size + 1}`, ...input, revokedAt: null })
      return refreshTokens.get(input.tokenHash)
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
    expect(result).toEqual({ ok: false, code: ErrorCode.EMAIL_ALREADY_REGISTERED, message: '邮箱已注册' })
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

    const login = await service.login({ email: 'user@example.com', password: 'password123', clientId: 'web' })
    expect(login.ok).toBe(true)
    if (!login.ok) throw new Error('login failed')

    const refresh = await service.refresh({ refreshToken: login.data.refresh_token, clientId: 'web' })
    expect(refresh.ok).toBe(true)
    if (refresh.ok) expect(refresh.data.refresh_token).not.toBe(login.data.refresh_token)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- auth.service.test.ts
```

Expected: FAIL because `auth.service.ts` does not exist or schemas are incomplete.

- [ ] **Step 3: Implement auth schemas**

Update `remote-server/src/modules/auth/auth.schemas.ts`:

```ts
import { z } from 'zod'

export const emailSchema = z.string().email()
export const passwordSchema = z.string().min(8).max(128)

export const authRegisterSchema = z.object({
  email: emailSchema,
  password: passwordSchema
})

export const authLoginSchema = z.object({
  email: emailSchema,
  password: passwordSchema
})

export const authRefreshSchema = z.object({
  refresh_token: z.string().min(32)
})

export const authLogoutSchema = z.object({
  refresh_token: z.string().min(32)
})

export type AuthRegisterInput = z.infer<typeof authRegisterSchema>
export type AuthLoginInput = z.infer<typeof authLoginSchema>
export type AuthRefreshInput = z.infer<typeof authRefreshSchema>
export type AuthLogoutInput = z.infer<typeof authLogoutSchema>
```

- [ ] **Step 4: Implement auth service with repository interface**

Create `remote-server/src/modules/auth/auth.service.ts`:

```ts
import { ErrorCode, type ErrorCodeValue } from '../../shared/errors.js'
import { createHash, createRandomToken } from '../../shared/crypto.js'
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
    role: user.role
  }
}

export function createAuthService(options: { repo: AuthRepository; config: AuthServiceConfig; clock?: Clock }) {
  const clock = options.clock ?? systemClock

  async function createSession(user: AuthUser, clientId: string, rotatedFromId: string | null = null) {
    const refreshToken = createRandomToken('rft')
    const now = clock.now()
    const record = await options.repo.createRefreshToken({
      userId: user.id,
      tokenHash: createHash(refreshToken, options.config.tokenPepper),
      clientId,
      expiresAt: addDays(now, options.config.refreshTokenTtlDays),
      revokedAt: null,
      rotatedFromId,
      createdAt: now
    })
    const accessToken = await createAccessToken(
      { userId: user.id, sessionId: record.id, role: user.role },
      { privateKeyPem: options.config.jwtPrivateKey, ttlSeconds: options.config.accessTokenTtlSeconds }
    )
    return {
      access_token: accessToken,
      refresh_token: refreshToken,
      expires_in: options.config.accessTokenTtlSeconds
    }
  }

  return {
    async register(input: { email: string; password: string }): Promise<ServiceSuccess<{ user: ReturnType<typeof publicUser> }> | ServiceFailure> {
      if (options.config.registrationMode !== 'open') {
        return { ok: false, code: ErrorCode.REGISTRATION_MODE_FORBIDDEN, message: '当前注册模式不允许自助注册' }
      }
      const existing = await options.repo.findUserByEmail(input.email)
      if (existing) return { ok: false, code: ErrorCode.EMAIL_ALREADY_REGISTERED, message: '邮箱已注册' }

      const now = clock.now()
      const password = await hashPassword(input.password)
      const user = await options.repo.createUser({
        email: input.email,
        passwordHash: password.hash,
        passwordAlgo: password.algo,
        role: 'user',
        status: 'active',
        createdAt: now,
        updatedAt: now,
        passwordUpdatedAt: now
      })
      return { ok: true, data: { user: publicUser(user) } }
    },

    async login(input: { email: string; password: string; clientId: string }) {
      const user = await options.repo.findUserByEmail(input.email)
      if (!user) return { ok: false as const, code: ErrorCode.ACCOUNT_NOT_FOUND, message: '账号不存在' }
      if (user.status !== 'active') return { ok: false as const, code: ErrorCode.ACCOUNT_DISABLED, message: '账号已禁用' }
      if (!(await verifyPassword(user.passwordHash, input.password))) {
        return { ok: false as const, code: ErrorCode.PASSWORD_INCORRECT, message: '密码错误' }
      }

      const session = await createSession(user, input.clientId)
      return { ok: true as const, data: { ...session, user: publicUser(user) } }
    },

    async refresh(input: { refreshToken: string; clientId: string }) {
      const tokenHash = createHash(input.refreshToken, options.config.tokenPepper)
      const record = await options.repo.findRefreshTokenByHash(tokenHash)
      if (!record || record.revokedAt) return { ok: false as const, code: ErrorCode.TOKEN_INVALID, message: 'Token 无效' }
      if (record.expiresAt.getTime() <= clock.now().getTime()) {
        return { ok: false as const, code: ErrorCode.TOKEN_EXPIRED, message: 'Token 已过期' }
      }
      const user = await options.repo.findUserById(record.userId)
      if (!user || user.status !== 'active') return { ok: false as const, code: ErrorCode.UNAUTHORIZED, message: '未登录' }

      await options.repo.revokeRefreshToken(record.id)
      const session = await createSession(user, input.clientId, record.id)
      return { ok: true as const, data: session }
    },

    async logout(input: { refreshToken: string }) {
      const record = await options.repo.findRefreshTokenByHash(createHash(input.refreshToken, options.config.tokenPepper))
      if (record) await options.repo.revokeRefreshToken(record.id)
      return { ok: true as const, data: {} }
    },

    async logoutAll(userId: string) {
      await options.repo.revokeAllRefreshTokens(userId)
      return { ok: true as const, data: {} }
    },

    async me(userId: string) {
      const user = await options.repo.findUserById(userId)
      if (!user || user.status !== 'active') return { ok: false as const, code: ErrorCode.UNAUTHORIZED, message: '未登录' }
      return { ok: true as const, data: { user: publicUser(user) } }
    }
  }
}
```

- [ ] **Step 5: Add Drizzle repository shell**

Create `remote-server/src/modules/auth/auth.repository.ts`:

```ts
import { eq } from 'drizzle-orm'
import { refreshTokens, users } from '../../db/schema.js'
import type { AuthRepository } from './auth.service.js'

export function createAuthRepository(db: any): AuthRepository {
  return {
    async findUserByEmail(email) {
      return (await db.select().from(users).where(eq(users.email, email)).limit(1))[0] ?? null
    },
    async findUserById(id) {
      return (await db.select().from(users).where(eq(users.id, id)).limit(1))[0] ?? null
    },
    async createUser(input) {
      return (await db.insert(users).values(input).returning())[0]
    },
    async createRefreshToken(input) {
      return (await db.insert(refreshTokens).values(input).returning())[0]
    },
    async findRefreshTokenByHash(tokenHash) {
      return (await db.select().from(refreshTokens).where(eq(refreshTokens.tokenHash, tokenHash)).limit(1))[0] ?? null
    },
    async revokeRefreshToken(id) {
      await db.update(refreshTokens).set({ revokedAt: new Date() }).where(eq(refreshTokens.id, id))
    },
    async revokeAllRefreshTokens(userId) {
      await db.update(refreshTokens).set({ revokedAt: new Date() }).where(eq(refreshTokens.userId, userId))
    }
  }
}
```

- [ ] **Step 6: Run auth service tests and build**

Run:

```bash
cd remote-server
npm test -- auth.service.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/auth/auth.schemas.ts remote-server/src/modules/auth/auth.service.ts remote-server/src/modules/auth/auth.repository.ts remote-server/tests/auth.service.test.ts
git commit -m "feat: 新增远程服务端认证业务服务" -m "修改内容：新增注册、登录、refresh token 轮换、退出和当前用户查询的业务规则与仓储接口。" -m "修改原因：Web 控制台和浏览器设备绑定流程需要账号登录态与 refresh token 生命周期管理。"
```

## Task 4: Auth Middleware And HTTP Routes

**Files:**
- Create: `remote-server/src/modules/auth/auth.middleware.ts`
- Create: `remote-server/src/modules/auth/auth.routes.ts`
- Modify: `remote-server/src/app.ts`
- Test: `remote-server/tests/auth.routes.test.ts`

- [ ] **Step 1: Write failing auth route tests**

Create `remote-server/tests/auth.routes.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'
import { apiFailure } from '../src/shared/response.js'
import { ErrorCode } from '../src/shared/errors.js'

describe('auth routes', () => {
  it('returns validation envelope for invalid register body', async () => {
    const app = buildApp({
      registerAuthRoutes: async (app) => {
        app.post('/api/v1/auth/register', async () => apiFailure(ErrorCode.BUSINESS_VALIDATION_FAILED, 'emailInvalid email；passwordString must contain at least 8 character(s)'))
      }
    })
    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/auth/register',
      payload: { email: 'bad', password: 'short' }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(100101)
  })

  it('requires bearer token for me', async () => {
    const app = buildApp({
      registerAuthRoutes: async (app) => {
        app.get('/api/v1/auth/me', async () => apiFailure(ErrorCode.UNAUTHORIZED, '未登录'))
      }
    })
    const response = await app.inject({
      method: 'GET',
      url: '/api/v1/auth/me'
    })

    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 200001,
      message: '未登录',
      data: null
    })
  })
})
```

- [ ] **Step 2: Run route tests to verify they fail**

Run:

```bash
cd remote-server
npm test -- auth.routes.test.ts
```

Expected: FAIL because auth routes are not registered.

- [ ] **Step 3: Implement auth middleware**

Create `remote-server/src/modules/auth/auth.middleware.ts`:

```ts
import type { FastifyRequest } from 'fastify'
import { ErrorCode } from '../../shared/errors.js'
import { apiFailure } from '../../shared/response.js'
import { verifyAccessToken } from './token.service.js'

export type AuthContext = {
  userId: string
  sessionId: string
  role: 'admin' | 'user'
}

export async function requireAuth(request: FastifyRequest, publicKeyPem: string): Promise<{ ok: true; auth: AuthContext } | { ok: false; response: unknown }> {
  const header = request.headers.authorization
  if (!header?.startsWith('Bearer ')) {
    return { ok: false, response: apiFailure(ErrorCode.UNAUTHORIZED, '未登录') }
  }

  try {
    return { ok: true, auth: await verifyAccessToken(header.slice('Bearer '.length), publicKeyPem) }
  } catch {
    return { ok: false, response: apiFailure(ErrorCode.TOKEN_INVALID, 'Token 无效') }
  }
}
```

- [ ] **Step 4: Implement auth routes**

Create `remote-server/src/modules/auth/auth.routes.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { ErrorCode } from '../../shared/errors.js'
import { apiFailure, apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { authLoginSchema, authLogoutSchema, authRefreshSchema, authRegisterSchema } from './auth.schemas.js'
import { requireAuth } from './auth.middleware.js'
import { createAuthRepository } from './auth.repository.js'
import { createAuthService } from './auth.service.js'

export async function registerAuthRoutes(app: FastifyInstance) {
  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const service = createAuthService({
    repo: createAuthRepository(db),
    config: {
      registrationMode: config.registrationMode,
      tokenPepper: config.tokenPepper,
      jwtPrivateKey: config.jwtPrivateKey,
      jwtPublicKey: config.jwtPublicKey,
      accessTokenTtlSeconds: config.accessTokenTtlSeconds,
      refreshTokenTtlDays: config.refreshTokenTtlDays
    }
  })

  app.post('/api/v1/auth/register', async (request) => {
    const parsed = parseBody(authRegisterSchema, request.body)
    if (!parsed.ok) return parsed.response
    const result = await service.register(parsed.data)
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.post('/api/v1/auth/login', async (request) => {
    const parsed = parseBody(authLoginSchema, request.body)
    if (!parsed.ok) return parsed.response
    const result = await service.login({ ...parsed.data, clientId: request.headers['user-agent'] ?? 'web' })
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.post('/api/v1/auth/refresh', async (request) => {
    const parsed = parseBody(authRefreshSchema, request.body)
    if (!parsed.ok) return parsed.response
    const result = await service.refresh({ refreshToken: parsed.data.refresh_token, clientId: request.headers['user-agent'] ?? 'web' })
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.post('/api/v1/auth/logout', async (request) => {
    const parsed = parseBody(authLogoutSchema, request.body)
    if (!parsed.ok) return parsed.response
    const result = await service.logout({ refreshToken: parsed.data.refresh_token })
    return apiSuccess(result.data)
  })

  app.post('/api/v1/auth/logout-all', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response
    const result = await service.logoutAll(auth.auth.userId)
    return apiSuccess(result.data)
  })

  app.get('/api/v1/auth/me', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response
    const result = await service.me(auth.auth.userId)
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })
}
```

- [ ] **Step 5: Register auth routes in app**

Update `remote-server/src/app.ts`:

```ts
import Fastify, { type FastifyInstance } from 'fastify'
import { ErrorCode } from './shared/errors.js'
import { apiFailure } from './shared/response.js'
import { registerHealthRoutes } from './modules/health/health.routes.js'
import { registerAuthRoutes } from './modules/auth/auth.routes.js'

export type AppDeps = {
  registerAuthRoutes?: (app: FastifyInstance) => Promise<void>
}

export function buildApp(deps: AppDeps = {}) {
  const app = Fastify({ logger: false })

  void registerHealthRoutes(app)
  void (deps.registerAuthRoutes ?? registerAuthRoutes)(app)

  app.setNotFoundHandler(async (_request, reply) => {
    return reply.status(404).send(apiFailure(ErrorCode.ROUTE_NOT_FOUND, '接口不存在'))
  })

  app.setErrorHandler(async (_error, _request, reply) => {
    return reply.status(500).send(apiFailure(ErrorCode.SYSTEM_ERROR, '系统异常'))
  })

  return app
}
```

The dependency hook keeps route-envelope tests isolated from PostgreSQL while production startup still registers real routes.

- [ ] **Step 6: Run route tests and build**

Run:

```bash
cd remote-server
npm test -- auth.routes.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/auth/auth.middleware.ts remote-server/src/modules/auth/auth.routes.ts remote-server/src/app.ts remote-server/tests/auth.routes.test.ts
git commit -m "feat: 新增远程服务端认证接口" -m "修改内容：新增注册、登录、刷新、退出、退出全部和当前用户 HTTP API。" -m "修改原因：远程 Web 控制台和浏览器绑定页需要统一账号登录接口。"
```

## Task 5: Desktop Login Service

**Files:**
- Create: `remote-server/src/modules/desktopLogin/desktopLogin.repository.ts`
- Create: `remote-server/src/modules/desktopLogin/desktopLogin.crypto.ts`
- Create: `remote-server/src/modules/desktopLogin/desktopLogin.service.ts`
- Create: `remote-server/src/modules/devices/devices.service.ts`
- Create: `remote-server/src/modules/devices/devices.repository.ts`
- Test: `remote-server/tests/desktopLogin.service.test.ts`

- [ ] **Step 1: Write failing desktop-login service tests**

Create `remote-server/tests/desktopLogin.service.test.ts`:

```ts
import { compactDecrypt, exportJWK, generateKeyPair } from 'jose'
import { describe, expect, it } from 'vitest'
import { ErrorCode } from '../src/shared/errors.js'
import { createDesktopLoginService, type DesktopLoginRepository } from '../src/modules/desktopLogin/desktopLogin.service.js'

function createRepo(): DesktopLoginRepository {
  const sessions = new Map<string, any>()
  const devices = new Map<string, any>()

  return {
    async createSession(input) {
      sessions.set(input.requestId, { id: `dls_${sessions.size + 1}`, ...input })
      return sessions.get(input.requestId)
    },
    async findSessionByRequestId(requestId) {
      return sessions.get(requestId) ?? null
    },
    async completeSession(requestId, input) {
      Object.assign(sessions.get(requestId), input)
    },
    async consumeSession(requestId) {
      sessions.get(requestId).status = 'consumed'
      sessions.get(requestId).consumedAt = new Date()
    },
    async findActiveDeviceByFingerprint(userId, fingerprintHash) {
      return [...devices.values()].find((device) => device.userId === userId && device.fingerprintHash === fingerprintHash && device.status === 'active') ?? null
    },
    async upsertDevice(input) {
      const existing = [...devices.values()].find((device) => device.userId === input.userId && device.fingerprintHash === input.fingerprintHash)
      if (existing) {
        Object.assign(existing, input)
        return existing
      }
      const device = { id: `dev_${devices.size + 1}`, ...input }
      devices.set(device.id, device)
      return device
    }
  }
}

describe('desktop login service', () => {
  it('starts session without leaking poll token in login url', async () => {
    const { publicKey } = await generateKeyPair('ECDH-ES')
    const desktopPublicKey = JSON.stringify(await exportJWK(publicKey))
    const service = createDesktopLoginService({
      repo: createRepo(),
      config: {
        publicUrl: 'https://remote.example.com',
        tokenPepper: 'pepper',
        desktopLoginTtlSeconds: 600
      }
    })

    const result = await service.start({
      device_name: 'NiuMa MacBook',
      device_fingerprint: 'fingerprint-1234567890',
      desktop_public_key: desktopPublicKey,
      capabilities: {
        agent_protocol_version: 1,
        rpc_protocol_version: 1,
        supports_webrtc: true,
        supports_relay: true,
        supports_remote_control: true
      }
    })

    expect(result.ok).toBe(true)
    if (!result.ok) throw new Error('start failed')
    expect(result.data.login_url).toContain(`request_id=${result.data.request_id}`)
    expect(result.data.login_url).not.toContain(result.data.poll_token)
  })

  it('returns pending before complete and consumed after completed poll', async () => {
    const { publicKey, privateKey } = await generateKeyPair('ECDH-ES')
    const desktopPublicKey = JSON.stringify(await exportJWK(publicKey))
    const repo = createRepo()
    const service = createDesktopLoginService({
      repo,
      config: {
        publicUrl: 'https://remote.example.com',
        tokenPepper: 'pepper',
        desktopLoginTtlSeconds: 600
      }
    })

    const start = await service.start({
      device_name: 'NiuMa MacBook',
      device_fingerprint: 'fingerprint-1234567890',
      desktop_public_key: desktopPublicKey,
      capabilities: {
        agent_protocol_version: 1,
        rpc_protocol_version: 1,
        supports_webrtc: true,
        supports_relay: true,
        supports_remote_control: true
      }
    })
    if (!start.ok) throw new Error('start failed')

    const pending = await service.poll({ request_id: start.data.request_id, poll_token: start.data.poll_token })
    expect(pending).toMatchObject({ ok: false, code: ErrorCode.DESKTOP_LOGIN_PENDING })

    await service.complete({
      requestId: start.data.request_id,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })
    const completed = await service.poll({ request_id: start.data.request_id, poll_token: start.data.poll_token })
    expect(completed.ok).toBe(true)
    if (!completed.ok) throw new Error('poll failed')
    const decrypted = await compactDecrypt(completed.data.encrypted_result.jwe, privateKey)
    expect(JSON.parse(new TextDecoder().decode(decrypted.plaintext)).device_token).toMatch(/^dvt_/)

    const consumed = await service.poll({ request_id: start.data.request_id, poll_token: start.data.poll_token })
    expect(consumed).toEqual({ ok: false, code: ErrorCode.DESKTOP_LOGIN_CONSUMED, message: '桌面登录会话已被消费' })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- desktopLogin.service.test.ts
```

Expected: FAIL because desktop-login service does not exist.

- [ ] **Step 3: Implement desktop-login result encryption**

Create `remote-server/src/modules/desktopLogin/desktopLogin.crypto.ts`:

```ts
import { CompactEncrypt, importJWK, type JWK } from 'jose'

export type DesktopLoginEncryptedResult = {
  alg: 'ECDH-ES+A256GCM'
  jwe: string
}

export async function encryptDesktopLoginResult(desktopPublicKeyJson: string, payload: object): Promise<DesktopLoginEncryptedResult> {
  const publicJwk = JSON.parse(desktopPublicKeyJson) as JWK
  const publicKey = await importJWK(publicJwk, 'ECDH-ES')
  const plaintext = new TextEncoder().encode(JSON.stringify(payload))
  const jwe = await new CompactEncrypt(plaintext)
    .setProtectedHeader({ alg: 'ECDH-ES', enc: 'A256GCM' })
    .encrypt(publicKey)

  return {
    alg: 'ECDH-ES+A256GCM',
    jwe
  }
}
```

- [ ] **Step 4: Implement desktop-login service**

Create `remote-server/src/modules/desktopLogin/desktopLogin.service.ts`:

```ts
import { ErrorCode, type ErrorCodeValue } from '../../shared/errors.js'
import { createHash, createRandomToken } from '../../shared/crypto.js'
import { createPublicId } from '../../shared/id.js'
import { addSeconds, secondsUntil, systemClock, type Clock } from '../../shared/time.js'
import type { DesktopLoginStartInput } from './desktopLogin.schemas.js'
import { encryptDesktopLoginResult, type DesktopLoginEncryptedResult } from './desktopLogin.crypto.js'

export type DesktopLoginSession = {
  id: string
  requestId: string
  pollTokenHash: string
  desktopPublicKey: string
  deviceName: string
  fingerprintHash: string
  capabilityJson: unknown
  status: 'pending' | 'completed' | 'consumed' | 'expired' | 'cancelled'
  userId: string | null
  deviceId: string | null
    encryptedResultJson: unknown | null
  expiresAt: Date
  completedAt: Date | null
  consumedAt: Date | null
  createdAt: Date
}

export type DesktopLoginRepository = {
  createSession(input: Omit<DesktopLoginSession, 'id'>): Promise<DesktopLoginSession>
  findSessionByRequestId(requestId: string): Promise<DesktopLoginSession | null>
  completeSession(requestId: string, input: Partial<DesktopLoginSession>): Promise<void>
  consumeSession(requestId: string): Promise<void>
  findActiveDeviceByFingerprint(userId: string, fingerprintHash: string): Promise<{ id: string; name: string } | null>
  upsertDevice(input: {
    userId: string
    name: string
    fingerprintHash: string
    tokenHash: string
    status: 'active'
    capabilityJson: unknown
    createdAt: Date
    updatedAt: Date
    revokedAt: Date | null
  }): Promise<{ id: string; name: string }>
}

export type DesktopLoginFailure = {
  ok: false
  code: ErrorCodeValue
  message: string
  data?: Record<string, unknown>
}

export function createDesktopLoginService(options: {
  repo: DesktopLoginRepository
  config: {
    publicUrl: string
    tokenPepper: string
    desktopLoginTtlSeconds: number
  }
  clock?: Clock
}) {
  const clock = options.clock ?? systemClock

  function isExpired(session: DesktopLoginSession) {
    return session.expiresAt.getTime() <= clock.now().getTime()
  }

  return {
    async start(input: DesktopLoginStartInput) {
      const requestId = createPublicId('dlr')
      const pollToken = createRandomToken('dlp')
      const now = clock.now()
      const fingerprintHash = createHash(input.device_fingerprint, options.config.tokenPepper)

      await options.repo.createSession({
        requestId,
        pollTokenHash: createHash(pollToken, options.config.tokenPepper),
        desktopPublicKey: input.desktop_public_key,
        deviceName: input.device_name,
        fingerprintHash,
        capabilityJson: input.capabilities,
        status: 'pending',
        userId: null,
        deviceId: null,
        encryptedResultJson: null,
        expiresAt: addSeconds(now, options.config.desktopLoginTtlSeconds),
        completedAt: null,
        consumedAt: null,
        createdAt: now
      })

      return {
        ok: true as const,
        data: {
          request_id: requestId,
          poll_token: pollToken,
          login_url: `${options.config.publicUrl}/desktop-login?request_id=${encodeURIComponent(requestId)}`,
          expires_in: options.config.desktopLoginTtlSeconds
        }
      }
    },

    async complete(input: { requestId: string; user: { id: string; email: string; role: 'admin' | 'user' } }) {
      const session = await options.repo.findSessionByRequestId(input.requestId)
      if (!session) return { ok: false as const, code: ErrorCode.DESKTOP_LOGIN_NOT_FOUND, message: '桌面登录会话不存在' }
      if (isExpired(session)) return { ok: false as const, code: ErrorCode.DESKTOP_LOGIN_EXPIRED, message: '桌面登录会话已过期' }
      if (session.status !== 'pending') return { ok: false as const, code: ErrorCode.DESKTOP_LOGIN_CONSUMED, message: '桌面登录会话已被消费' }

      const deviceToken = createRandomToken('dvt')
      const device = await options.repo.upsertDevice({
        userId: input.user.id,
        name: session.deviceName,
        fingerprintHash: session.fingerprintHash,
        tokenHash: createHash(deviceToken, options.config.tokenPepper),
        status: 'active',
        capabilityJson: session.capabilityJson,
        createdAt: clock.now(),
        updatedAt: clock.now(),
        revokedAt: null
      })

      const encryptedResult = await encryptDesktopLoginResult(session.desktopPublicKey, {
        user: input.user,
        device,
        device_token: deviceToken
      })

      await options.repo.completeSession(input.requestId, {
        status: 'completed',
        userId: input.user.id,
        deviceId: device.id,
        encryptedResultJson: encryptedResult,
        completedAt: clock.now()
      })
      return { ok: true as const, data: {} }
    },

    async poll(input: { request_id: string; poll_token: string }) {
      const session = await options.repo.findSessionByRequestId(input.request_id)
      if (!session) return { ok: false as const, code: ErrorCode.DESKTOP_LOGIN_NOT_FOUND, message: '桌面登录会话不存在' }
      if (session.pollTokenHash !== createHash(input.poll_token, options.config.tokenPepper)) {
        return { ok: false as const, code: ErrorCode.DESKTOP_LOGIN_POLL_TOKEN_INVALID, message: '桌面登录轮询 token 无效' }
      }
      if (isExpired(session)) return { ok: false as const, code: ErrorCode.DESKTOP_LOGIN_EXPIRED, message: '桌面登录会话已过期' }
      if (session.status === 'pending') {
        return {
          ok: false as const,
          code: ErrorCode.DESKTOP_LOGIN_PENDING,
          message: '桌面登录会话尚未完成',
          data: { status: 'pending', expires_in: secondsUntil(clock.now(), session.expiresAt) }
        }
      }
      if (session.status === 'consumed') {
        return { ok: false as const, code: ErrorCode.DESKTOP_LOGIN_CONSUMED, message: '桌面登录会话已被消费' }
      }

      await options.repo.consumeSession(input.request_id)
      return {
        ok: true as const,
        data: {
          encrypted_result: session.encryptedResultJson as DesktopLoginEncryptedResult
        }
      }
    }
  }
}
```

- [ ] **Step 5: Implement desktop-login repository**

Create `remote-server/src/modules/desktopLogin/desktopLogin.repository.ts`:

```ts
import { and, eq } from 'drizzle-orm'
import { desktopLoginSessions, devices } from '../../db/schema.js'
import type { DesktopLoginRepository } from './desktopLogin.service.js'

export function createDesktopLoginRepository(db: any): DesktopLoginRepository {
  return {
    async createSession(input) {
      return (await db.insert(desktopLoginSessions).values(input).returning())[0]
    },
    async findSessionByRequestId(requestId) {
      return (await db.select().from(desktopLoginSessions).where(eq(desktopLoginSessions.requestId, requestId)).limit(1))[0] ?? null
    },
    async completeSession(requestId, input) {
      await db.update(desktopLoginSessions).set(input).where(eq(desktopLoginSessions.requestId, requestId))
    },
    async consumeSession(requestId) {
      await db.update(desktopLoginSessions).set({ status: 'consumed', consumedAt: new Date() }).where(eq(desktopLoginSessions.requestId, requestId))
    },
    async findActiveDeviceByFingerprint(userId, fingerprintHash) {
      return (await db
        .select()
        .from(devices)
        .where(and(eq(devices.userId, userId), eq(devices.fingerprintHash, fingerprintHash), eq(devices.status, 'active')))
        .limit(1))[0] ?? null
    },
    async upsertDevice(input) {
      const existing = await this.findActiveDeviceByFingerprint(input.userId, input.fingerprintHash)
      if (existing) {
        return (await db.update(devices).set(input).where(eq(devices.id, existing.id)).returning())[0]
      }
      return (await db.insert(devices).values(input).returning())[0]
    }
  }
}
```

- [ ] **Step 6: Run service tests and build**

Run:

```bash
cd remote-server
npm test -- desktopLogin.service.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/modules/desktopLogin/desktopLogin.crypto.ts remote-server/src/modules/desktopLogin/desktopLogin.repository.ts remote-server/src/modules/desktopLogin/desktopLogin.service.ts remote-server/tests/desktopLogin.service.test.ts
git commit -m "feat: 新增桌面浏览器登录绑定服务" -m "修改内容：新增 desktop-login start、complete、poll 业务规则和仓储实现。" -m "修改原因：本机 NiumaNotifier 需要通过浏览器登录完成账号绑定并获取设备凭据。"
```

## Task 6: Desktop Login Routes

**Files:**
- Create: `remote-server/src/modules/desktopLogin/desktopLogin.routes.ts`
- Modify: `remote-server/src/app.ts`
- Test: `remote-server/tests/desktopLogin.routes.test.ts`

- [ ] **Step 1: Write failing route tests**

Create `remote-server/tests/desktopLogin.routes.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'

describe('desktop login routes', () => {
  it('validates start request', async () => {
    const app = buildApp()
    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/desktop-login/start',
      payload: {
        device_name: '',
        device_fingerprint: 'short',
        desktop_public_key: 'short',
        capabilities: {}
      }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(100101)
  })

  it('requires login for complete', async () => {
    const app = buildApp()
    const response = await app.inject({
      method: 'POST',
      url: '/api/v1/desktop-login/complete',
      payload: { request_id: 'dlr_123' }
    })

    expect(response.statusCode).toBe(200)
    expect(response.json().code).toBe(200001)
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- desktopLogin.routes.test.ts
```

Expected: FAIL because routes are not registered.

- [ ] **Step 3: Implement desktop-login routes**

Create `remote-server/src/modules/desktopLogin/desktopLogin.routes.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { apiFailure, apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { requireAuth } from '../auth/auth.middleware.js'
import { createAuthRepository } from '../auth/auth.repository.js'
import { createDesktopLoginRepository } from './desktopLogin.repository.js'
import { createDesktopLoginService } from './desktopLogin.service.js'
import { desktopLoginCompleteSchema, desktopLoginPollSchema, desktopLoginStartSchema } from './desktopLogin.schemas.js'

export async function registerDesktopLoginRoutes(app: FastifyInstance) {
  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const authRepo = createAuthRepository(db)
  const service = createDesktopLoginService({
    repo: createDesktopLoginRepository(db),
    config: {
      publicUrl: config.publicUrl,
      tokenPepper: config.tokenPepper,
      desktopLoginTtlSeconds: 600
    }
  })

  app.post('/api/v1/desktop-login/start', async (request) => {
    const parsed = parseBody(desktopLoginStartSchema, request.body)
    if (!parsed.ok) return parsed.response
    const result = await service.start(parsed.data)
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message, result.data ?? null)
  })

  app.post('/api/v1/desktop-login/complete', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response
    const parsed = parseBody(desktopLoginCompleteSchema, request.body)
    if (!parsed.ok) return parsed.response
    const currentUser = await authRepo.findUserById(auth.auth.userId)
    if (!currentUser || currentUser.status !== 'active') return apiFailure(ErrorCode.UNAUTHORIZED, '未登录')
    const result = await service.complete({
      requestId: parsed.data.request_id,
      user: {
        id: currentUser.id,
        email: currentUser.email,
        role: currentUser.role
      }
    })
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.post('/api/v1/desktop-login/poll', async (request) => {
    const parsed = parseBody(desktopLoginPollSchema, request.body)
    if (!parsed.ok) return parsed.response
    const result = await service.poll(parsed.data)
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message, result.data ?? null)
  })
}
```

- [ ] **Step 4: Register desktop-login routes**

Update `remote-server/src/app.ts`:

```ts
import { registerDesktopLoginRoutes } from './modules/desktopLogin/desktopLogin.routes.js'
```

Inside `buildApp()` after auth route registration:

```ts
  void registerDesktopLoginRoutes(app)
```

- [ ] **Step 5: Run route tests and build**

Run:

```bash
cd remote-server
npm test -- desktopLogin.routes.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/modules/desktopLogin/desktopLogin.routes.ts remote-server/src/app.ts remote-server/tests/desktopLogin.routes.test.ts
git commit -m "feat: 新增桌面浏览器登录绑定接口" -m "修改内容：新增 desktop-login start、complete、poll HTTP API 并接入统一响应结构。" -m "修改原因：本机设置页点击登录后需要通过浏览器完成账号登录和设备绑定。"
```

## Task 7: Device List API

**Files:**
- Create: `remote-server/src/modules/devices/devices.repository.ts`
- Create: `remote-server/src/modules/devices/devices.service.ts`
- Create: `remote-server/src/modules/devices/devices.routes.ts`
- Modify: `remote-server/src/modules/devices/devices.schemas.ts`
- Modify: `remote-server/src/app.ts`
- Test: `remote-server/tests/devices.routes.test.ts`

- [ ] **Step 1: Write failing device route tests**

Create `remote-server/tests/devices.routes.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'

describe('devices routes', () => {
  it('requires auth for device list', async () => {
    const app = buildApp()
    const response = await app.inject({
      method: 'GET',
      url: '/api/v1/devices/list'
    })

    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 200001,
      message: '未登录',
      data: null
    })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- devices.routes.test.ts
```

Expected: FAIL because devices route is not registered.

- [ ] **Step 3: Implement device schemas**

Update `remote-server/src/modules/devices/devices.schemas.ts`:

```ts
import { z } from 'zod'
import { deviceCapabilitiesSchema } from '../desktopLogin/desktopLogin.schemas.js'

export const deviceIdSchema = z.string().min(1).max(160)

export const deviceRenameSchema = z.object({
  device_id: deviceIdSchema,
  name: z.string().min(1).max(120)
})

export const deviceRegisterSchema = z.object({
  device_name: z.string().min(1).max(120),
  device_fingerprint: z.string().min(32).max(128),
  capabilities: deviceCapabilitiesSchema
})
```

- [ ] **Step 4: Implement devices service and repository**

Create `remote-server/src/modules/devices/devices.service.ts`:

```ts
export type DeviceListItem = {
  id: string
  name: string
  online: boolean
  last_seen_at: string | null
  capabilities: unknown
}

export type DevicesRepository = {
  listActiveDevices(userId: string): Promise<Array<{
    id: string
    name: string
    lastSeenAt: Date | null
    capabilityJson: unknown
  }>>
}

export function createDevicesService(options: { repo: DevicesRepository }) {
  return {
    async list(userId: string): Promise<{ list: DeviceListItem[] }> {
      const devices = await options.repo.listActiveDevices(userId)
      return {
        list: devices.map((device) => ({
          id: device.id,
          name: device.name,
          online: false,
          last_seen_at: device.lastSeenAt?.toISOString() ?? null,
          capabilities: device.capabilityJson
        }))
      }
    }
  }
}
```

Create `remote-server/src/modules/devices/devices.repository.ts`:

```ts
import { and, eq } from 'drizzle-orm'
import { devices } from '../../db/schema.js'
import type { DevicesRepository } from './devices.service.js'

export function createDevicesRepository(db: any): DevicesRepository {
  return {
    async listActiveDevices(userId) {
      return db.select().from(devices).where(and(eq(devices.userId, userId), eq(devices.status, 'active')))
    }
  }
}
```

`online` is fixed to `false` in this slice because Redis presence and `/ws/device` are intentionally left for the WebSocket plan. The route shape is stable now; online status becomes real when presence exists.

- [ ] **Step 5: Implement devices routes**

Create `remote-server/src/modules/devices/devices.routes.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { apiSuccess } from '../../shared/response.js'
import { requireAuth } from '../auth/auth.middleware.js'
import { createDevicesRepository } from './devices.repository.js'
import { createDevicesService } from './devices.service.js'

export async function registerDevicesRoutes(app: FastifyInstance) {
  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const service = createDevicesService({
    repo: createDevicesRepository(db)
  })

  app.get('/api/v1/devices/list', async (request) => {
    const auth = await requireAuth(request, config.jwtPublicKey)
    if (!auth.ok) return auth.response
    return apiSuccess(await service.list(auth.auth.userId))
  })
}
```

- [ ] **Step 6: Register device routes**

Update `remote-server/src/app.ts`:

```ts
import { registerDevicesRoutes } from './modules/devices/devices.routes.js'
```

Inside `buildApp()` after desktop-login route registration:

```ts
  void registerDevicesRoutes(app)
```

- [ ] **Step 7: Run route tests and build**

Run:

```bash
cd remote-server
npm test -- devices.routes.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 8: Commit**

```bash
git add remote-server/src/modules/devices/devices.repository.ts remote-server/src/modules/devices/devices.service.ts remote-server/src/modules/devices/devices.routes.ts remote-server/src/modules/devices/devices.schemas.ts remote-server/src/app.ts remote-server/tests/devices.routes.test.ts
git commit -m "feat: 新增远程服务端设备列表接口" -m "修改内容：新增已绑定设备列表 API，返回设备名、能力摘要和当前在线状态字段。" -m "修改原因：远程 Web 控制台登录后需要查看当前账号已绑定设备。"
```

## Task 8: Full Milestone Verification

**Files:**
- Verify all files from Tasks 1-7.

- [ ] **Step 1: Run full checks**

Run:

```bash
cd remote-server
npm run check
```

Expected: TypeScript build passes and all Vitest tests pass.

- [ ] **Step 2: Verify API paths do not use dynamic path params**

Run:

```bash
rg -n "app\\.(get|post|put|delete)\\('/api/[^']*:" remote-server/src
```

Expected: no output.

- [ ] **Step 3: Verify device and refresh tokens are not stored as plaintext**

Run:

```bash
rg -n "refresh_token|device_token|poll_token" remote-server/src/db remote-server/src/modules
```

Expected: route response fields may appear, but schema and repository persistence use `tokenHash`, `pollTokenHash`, or `encryptedResultJson`; no database column stores plaintext `refresh_token`, `device_token`, or `poll_token`.

- [ ] **Step 4: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

- [ ] **Step 5: Record milestone result**

Add this note to the implementation issue or PR description:

```text
Remote server auth and desktop-login slice complete:
- Email/password register and login
- Access token and refresh token rotation
- Logout and logout-all
- Current user API
- Desktop-login start/complete/poll
- Device list API

Verification:
- cd remote-server && npm run check
- rg API dynamic path param scan
- rg plaintext token persistence scan
```

Do not mark remote server complete after this milestone. `/ws/device`, Redis presence, device unbind/revoke, connection creation, signaling, relay, and E2EE RPC session encryption are still separate milestones.

## Self-Review

Spec coverage in this plan:

- Email/password register and login: covered by Tasks 2-4.
- Access token and refresh token model: covered by Tasks 1-4.
- Refresh token rotation: covered by Task 3.
- Logout current session and logout all sessions: covered by Tasks 3-4.
- `GET /api/v1/auth/me`: covered by Task 4.
- Desktop login `start`, `complete`, and `poll`: covered by Tasks 5-6.
- `poll_token` excluded from browser URL: covered by Task 5 tests.
- Device token stored as hash and returned only in encrypted result envelope: covered by Task 5 and Task 8 scan.
- Device list after login: covered by Task 7.
- Unified API envelope and no dynamic path params: covered by Tasks 4, 6, 7, and Task 8 verification.

Known follow-up plans:

- Implement admin bootstrap and invite/admin-created users for `REGISTRATION_MODE=admin_invite`.
- Implement device rename, unbind, and revoke-token.
- Implement Redis presence and `/ws/device`.
- Implement connection creation, signaling, relay, and E2EE RPC.
