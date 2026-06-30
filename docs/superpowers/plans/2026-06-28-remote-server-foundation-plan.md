# Remote Server Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first testable remote-server slice: TypeScript/Fastify service skeleton, standard API envelope, config, health check, auth/device schema foundation, desktop browser-login binding API contract, and Docker Compose self-hosting skeleton.

**Architecture:** This plan creates `remote-server/` as a modular TypeScript service inside the existing repository. It does not implement WebRTC, relay, Web console UI, or the Rust RemoteAgent; it establishes the server foundation and the browser-based device binding contract those later slices depend on.

**Tech Stack:** Node.js LTS, TypeScript, Fastify, Zod, Drizzle ORM, PostgreSQL, Redis, Vitest, Docker Compose. Host-facing service port uses `27880`; PostgreSQL and Redis stay internal to Docker and are not mapped to host default ports.

---

## Scope Check

The remote access feature spans four independent subsystems:

- Remote server.
- Remote Web console.
- Local NiumaNotifier RemoteAgent.
- WebRTC/E2EE transport integration.

This plan covers only the remote-server foundation. It produces working, testable software with stable API response conventions, config loading, database schema declarations, desktop-login API shape, and Docker deployment scaffolding. Subsequent plans should build:

- `remote-web-client-implementation-plan.md`
- `remote-agent-implementation-plan.md`
- `remote-transport-e2ee-implementation-plan.md`
- `remote-control-rpc-implementation-plan.md`

## File Structure

Create:

- `remote-server/package.json` - service package scripts and dependencies.
- `remote-server/tsconfig.json` - strict TypeScript config.
- `remote-server/vitest.config.ts` - test config.
- `remote-server/drizzle.config.ts` - migration config.
- `remote-server/.env.example` - self-hosting config with non-default host-facing ports.
- `remote-server/Dockerfile` - multi-stage production image.
- `remote-server/docker-compose.yml` - self-hosting stack.
- `remote-server/migrations/.gitkeep` - keep the migrations directory present before generated SQL exists.
- `remote-server/src/config.ts` - env parsing.
- `remote-server/src/app.ts` - Fastify app composition.
- `remote-server/src/server.ts` - server entrypoint.
- `remote-server/src/db/schema.ts` - Drizzle table definitions.
- `remote-server/src/db/client.ts` - Drizzle client factory.
- `remote-server/src/db/migrate.ts` - migration runner.
- `remote-server/src/shared/response.ts` - standard envelope helpers.
- `remote-server/src/shared/errors.ts` - error code registry.
- `remote-server/src/shared/validation.ts` - Zod validation to envelope conversion.
- `remote-server/src/shared/crypto.ts` - token hashing and random token helpers.
- `remote-server/src/modules/health/health.routes.ts` - health endpoint.
- `remote-server/src/modules/auth/auth.schemas.ts` - auth request schemas.
- `remote-server/src/modules/devices/devices.schemas.ts` - device request schemas.
- `remote-server/src/modules/desktopLogin/desktopLogin.schemas.ts` - desktop login request schemas.
- `remote-server/tests/response.test.ts`
- `remote-server/tests/config.test.ts`
- `remote-server/tests/health.test.ts`
- `remote-server/tests/schema.test.ts`
- `remote-server/tests/desktopLogin.schemas.test.ts`

Modify:

- Root `package.json` only if adding convenience scripts is desired in this plan. If changed, use scripts that do not start services on default ports.

## Task 1: Scaffold Remote Server Package

**Files:**
- Create: `remote-server/package.json`
- Create: `remote-server/tsconfig.json`
- Create: `remote-server/vitest.config.ts`

- [ ] **Step 1: Write package skeleton**

Create `remote-server/package.json`:

```json
{
  "name": "niuma-remote-server",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "tsx watch src/server.ts",
    "build": "tsc -p tsconfig.json",
    "test": "vitest run",
    "check": "npm run build && npm test",
    "db:generate": "drizzle-kit generate",
    "db:migrate": "tsx src/db/migrate.ts"
  },
  "dependencies": {
    "@fastify/websocket": "^11.0.2",
    "@node-rs/argon2": "^2.0.2",
    "drizzle-orm": "^0.39.3",
    "fastify": "^5.2.1",
    "ioredis": "^5.4.2",
    "jose": "^5.9.6",
    "pg": "^8.13.1",
    "zod": "^3.24.1"
  },
  "devDependencies": {
    "@types/node": "^22.10.2",
    "@types/pg": "^8.11.10",
    "drizzle-kit": "^0.30.2",
    "tsx": "^4.19.2",
    "typescript": "^5.7.2",
    "vitest": "^2.1.8"
  }
}
```

- [ ] **Step 2: Add TypeScript config**

Create `remote-server/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "esModuleInterop": true,
    "forceConsistentCasingInFileNames": true,
    "skipLibCheck": true,
    "outDir": "dist",
    "rootDir": "src",
    "types": ["node"]
  },
  "include": ["src/**/*.ts"],
  "exclude": ["dist", "node_modules"]
}
```

- [ ] **Step 3: Add Vitest config**

Create `remote-server/vitest.config.ts`:

```ts
import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    environment: 'node',
    include: ['tests/**/*.test.ts']
  }
})
```

- [ ] **Step 4: Install dependencies**

Run:

```bash
cd remote-server
npm install
```

Expected: `package-lock.json` is created under `remote-server/`, install exits 0.

- [ ] **Step 5: Run baseline check**

Run:

```bash
cd remote-server
npm test
```

Expected: Vitest reports no test files or exits with a clear no-tests status. If Vitest exits non-zero only because no tests exist, proceed after Task 2 adds the first tests.

- [ ] **Step 6: Commit scaffold**

```bash
git add remote-server/package.json remote-server/package-lock.json remote-server/tsconfig.json remote-server/vitest.config.ts
git commit -m "feat: 新增远程服务端项目骨架" -m "修改内容：新增 remote-server TypeScript 包、构建脚本和测试配置。" -m "修改原因：为远程服务端账号、设备和连接接口提供独立实现入口。"
```

## Task 2: Standard API Envelope And Error Registry

**Files:**
- Create: `remote-server/src/shared/errors.ts`
- Create: `remote-server/src/shared/response.ts`
- Create: `remote-server/tests/response.test.ts`

- [ ] **Step 1: Write failing envelope tests**

Create `remote-server/tests/response.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { apiFailure, apiSuccess } from '../src/shared/response.js'
import { ErrorCode } from '../src/shared/errors.js'

describe('standard API envelope', () => {
  it('returns success envelope with object data', () => {
    expect(apiSuccess({ service: 'remote' })).toEqual({
      code: 0,
      message: 'ok',
      data: { service: 'remote' }
    })
  })

  it('returns failure envelope with outer code and message', () => {
    expect(apiFailure(ErrorCode.UNAUTHORIZED, '未登录')).toEqual({
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
npm test -- response.test.ts
```

Expected: FAIL because `src/shared/response.ts` and `src/shared/errors.ts` do not exist.

- [ ] **Step 3: Implement errors**

Create `remote-server/src/shared/errors.ts`:

```ts
export const ErrorCode = {
  OK: 0,
  PROTOCOL_PARAM_ERROR: 100001,
  PROTOCOL_MISSING_REQUIRED: 100002,
  PROTOCOL_PARAM_TYPE: 100003,
  PROTOCOL_PARAM_FORMAT: 100004,
  BUSINESS_VALIDATION_FAILED: 100101,
  UNAUTHORIZED: 200001,
  TOKEN_INVALID: 200002,
  TOKEN_EXPIRED: 200003,
  FORBIDDEN: 200004,
  EMAIL_FORMAT_INVALID: 200101,
  PASSWORD_FORMAT_INVALID: 200102,
  ACCOUNT_NOT_FOUND: 200401,
  PASSWORD_INCORRECT: 200402,
  ACCOUNT_DISABLED: 200403,
  EMAIL_ALREADY_REGISTERED: 200501,
  DEVICE_NOT_FOUND: 210401,
  DEVICE_REVOKED: 210402,
  DEVICE_FORBIDDEN: 210403,
  DEVICE_OFFLINE: 210404,
  DEVICE_TOKEN_INVALID: 210405,
  DEVICE_TOKEN_REVOKED: 210406,
  CONNECTION_NOT_FOUND: 220401,
  CONNECTION_EXPIRED: 220402,
  CONNECTION_FORBIDDEN: 220403,
  REMOTE_DEVICE_UNREACHABLE: 220404,
  SIGNALING_SESSION_NOT_FOUND: 220405,
  RELAY_SESSION_NOT_FOUND: 220406,
  DESKTOP_LOGIN_NOT_FOUND: 240401,
  DESKTOP_LOGIN_EXPIRED: 240402,
  DESKTOP_LOGIN_POLL_TOKEN_INVALID: 240403,
  DESKTOP_LOGIN_PENDING: 240404,
  DESKTOP_LOGIN_CONSUMED: 240405,
  SYSTEM_ERROR: 900001,
  DATABASE_ERROR: 900002,
  DOWNSTREAM_ERROR: 900003,
  SERVICE_UNAVAILABLE: 900004,
  ROUTE_NOT_FOUND: 900005
} as const

export type ErrorCodeValue = (typeof ErrorCode)[keyof typeof ErrorCode]
```

- [ ] **Step 4: Implement response helpers**

Create `remote-server/src/shared/response.ts`:

```ts
import type { ErrorCodeValue } from './errors.js'

export type ApiEnvelope<T extends object = Record<string, unknown>> = {
  code: number
  message: string
  data: T | null
}

export function apiSuccess<T extends object>(data: T): ApiEnvelope<T> {
  return { code: 0, message: 'ok', data }
}

export function apiFailure(
  code: ErrorCodeValue,
  message: string,
  data: Record<string, unknown> | null = null
): ApiEnvelope {
  return { code, message, data }
}
```

- [ ] **Step 5: Run test to verify it passes**

Run:

```bash
cd remote-server
npm test -- response.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/shared/errors.ts remote-server/src/shared/response.ts remote-server/tests/response.test.ts
git commit -m "feat: 新增远程服务端统一响应结构" -m "修改内容：新增 API envelope helper 和远程服务端错误码台账。" -m "修改原因：远程服务端普通业务接口必须遵循统一 code/message/data 响应规范。"
```

## Task 3: Config Loader With Non-Default Ports

**Files:**
- Create: `remote-server/src/config.ts`
- Create: `remote-server/tests/config.test.ts`
- Create: `remote-server/.env.example`

- [ ] **Step 1: Write failing config tests**

Create `remote-server/tests/config.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { loadConfigFromEnv } from '../src/config.js'

describe('remote server config', () => {
  it('uses project-specific host-facing port by default', () => {
    const config = loadConfigFromEnv({
      DATABASE_URL: 'postgres://niuma:pw@postgres:5432/niuma_remote',
      REDIS_URL: 'redis://redis:6379',
      JWT_PRIVATE_KEY: 'private',
      JWT_PUBLIC_KEY: 'public',
      TOKEN_PEPPER: 'pepper'
    })

    expect(config.port).toBe(27880)
    expect(config.bind).toBe('0.0.0.0')
    expect(config.registrationMode).toBe('admin_invite')
  })

  it('rejects default host-facing application ports', () => {
    expect(() =>
      loadConfigFromEnv({
        REMOTE_SERVER_PORT: '8080',
        DATABASE_URL: 'postgres://niuma:pw@postgres:5432/niuma_remote',
        REDIS_URL: 'redis://redis:6379',
        JWT_PRIVATE_KEY: 'private',
        JWT_PUBLIC_KEY: 'public',
        TOKEN_PEPPER: 'pepper'
      })
    ).toThrow('REMOTE_SERVER_PORT 不能使用常见默认端口')
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- config.test.ts
```

Expected: FAIL because `src/config.ts` does not exist.

- [ ] **Step 3: Implement config loader**

Create `remote-server/src/config.ts`:

```ts
import { z } from 'zod'

const blockedHostPorts = new Set([80, 443, 3000, 5000, 5173, 8000, 8080, 5432, 6379])

const envSchema = z.object({
  REMOTE_SERVER_PUBLIC_URL: z.string().url().default('http://127.0.0.1:27880'),
  REMOTE_SERVER_BIND: z.string().default('0.0.0.0'),
  REMOTE_SERVER_PORT: z.coerce.number().int().positive().default(27880),
  DATABASE_URL: z.string().min(1),
  REDIS_URL: z.string().min(1),
  JWT_PRIVATE_KEY: z.string().min(1),
  JWT_PUBLIC_KEY: z.string().min(1),
  TOKEN_PEPPER: z.string().min(1),
  ACCESS_TOKEN_TTL_SECONDS: z.coerce.number().int().positive().default(900),
  REFRESH_TOKEN_TTL_DAYS: z.coerce.number().int().positive().default(30),
  CONNECTION_TOKEN_TTL_SECONDS: z.coerce.number().int().positive().default(120),
  REGISTRATION_MODE: z.enum(['open', 'admin_invite', 'disabled']).default('admin_invite'),
  TURN_ENABLED: z.coerce.boolean().default(false),
  TURN_URLS: z.string().default(''),
  TURN_USERNAME: z.string().default(''),
  TURN_CREDENTIAL: z.string().default('')
})

export type RemoteServerConfig = ReturnType<typeof loadConfigFromEnv>

export function loadConfigFromEnv(env: NodeJS.ProcessEnv = process.env) {
  const parsed = envSchema.parse(env)

  if (blockedHostPorts.has(parsed.REMOTE_SERVER_PORT)) {
    throw new Error('REMOTE_SERVER_PORT 不能使用常见默认端口')
  }

  return {
    publicUrl: parsed.REMOTE_SERVER_PUBLIC_URL,
    bind: parsed.REMOTE_SERVER_BIND,
    port: parsed.REMOTE_SERVER_PORT,
    databaseUrl: parsed.DATABASE_URL,
    redisUrl: parsed.REDIS_URL,
    jwtPrivateKey: parsed.JWT_PRIVATE_KEY,
    jwtPublicKey: parsed.JWT_PUBLIC_KEY,
    tokenPepper: parsed.TOKEN_PEPPER,
    accessTokenTtlSeconds: parsed.ACCESS_TOKEN_TTL_SECONDS,
    refreshTokenTtlDays: parsed.REFRESH_TOKEN_TTL_DAYS,
    connectionTokenTtlSeconds: parsed.CONNECTION_TOKEN_TTL_SECONDS,
    registrationMode: parsed.REGISTRATION_MODE,
    turn: {
      enabled: parsed.TURN_ENABLED,
      urls: parsed.TURN_URLS.split(',').map((item) => item.trim()).filter(Boolean),
      username: parsed.TURN_USERNAME,
      credential: parsed.TURN_CREDENTIAL
    }
  }
}
```

- [ ] **Step 4: Add environment example**

Create `remote-server/.env.example`:

```env
REMOTE_SERVER_PUBLIC_URL=https://remote.example.com
REMOTE_SERVER_BIND=0.0.0.0
REMOTE_SERVER_PORT=27880

DATABASE_URL=postgres://niuma:change-me@postgres:5432/niuma_remote
REDIS_URL=redis://redis:6379

JWT_PRIVATE_KEY=
JWT_PUBLIC_KEY=
TOKEN_PEPPER=

ACCESS_TOKEN_TTL_SECONDS=900
REFRESH_TOKEN_TTL_DAYS=30
CONNECTION_TOKEN_TTL_SECONDS=120

REGISTRATION_MODE=admin_invite
BOOTSTRAP_ADMIN_EMAIL=admin@example.com
BOOTSTRAP_ADMIN_PASSWORD=change-me

TURN_ENABLED=false
TURN_URLS=
TURN_USERNAME=
TURN_CREDENTIAL=
```

- [ ] **Step 5: Run config tests**

Run:

```bash
cd remote-server
npm test -- config.test.ts
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add remote-server/src/config.ts remote-server/tests/config.test.ts remote-server/.env.example
git commit -m "feat: 新增远程服务端配置加载" -m "修改内容：新增环境变量解析、非默认端口校验和自托管 env 示例。" -m "修改原因：远程服务端需要稳定配置入口，并避免占用常见默认宿主端口。"
```

## Task 4: Validation Helper And Health Endpoint

**Files:**
- Create: `remote-server/src/shared/validation.ts`
- Create: `remote-server/src/modules/health/health.routes.ts`
- Create: `remote-server/src/app.ts`
- Create: `remote-server/src/server.ts`
- Create: `remote-server/tests/health.test.ts`

- [ ] **Step 1: Write failing health tests**

Create `remote-server/tests/health.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'

describe('health route', () => {
  it('returns standard envelope', async () => {
    const app = buildApp()
    const response = await app.inject({ method: 'GET', url: '/api/v1/health' })
    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 0,
      message: 'ok',
      data: {
        service: 'niuma-remote-server',
        status: 'ok'
      }
    })
  })

  it('returns standard envelope for missing route', async () => {
    const app = buildApp()
    const response = await app.inject({ method: 'GET', url: '/missing' })
    expect(response.statusCode).toBe(404)
    expect(response.json()).toEqual({
      code: 900005,
      message: '接口不存在',
      data: null
    })
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- health.test.ts
```

Expected: FAIL because app and route files do not exist.

- [ ] **Step 3: Implement validation helper**

Create `remote-server/src/shared/validation.ts`:

```ts
import type { ZodError, ZodSchema } from 'zod'
import { ErrorCode } from './errors.js'
import { apiFailure, type ApiEnvelope } from './response.js'

export function formatZodError(error: ZodError): string {
  return error.issues
    .map((issue) => `${issue.path.join('.') || 'body'}${issue.message}`)
    .join('；')
}

export function parseBody<T>(schema: ZodSchema<T>, value: unknown): { ok: true; data: T } | { ok: false; response: ApiEnvelope } {
  const parsed = schema.safeParse(value)
  if (parsed.success) return { ok: true, data: parsed.data }
  return {
    ok: false,
    response: apiFailure(ErrorCode.BUSINESS_VALIDATION_FAILED, formatZodError(parsed.error))
  }
}
```

- [ ] **Step 4: Implement health route**

Create `remote-server/src/modules/health/health.routes.ts`:

```ts
import type { FastifyInstance } from 'fastify'
import { apiSuccess } from '../../shared/response.js'

export async function registerHealthRoutes(app: FastifyInstance) {
  app.get('/api/v1/health', async () =>
    apiSuccess({
      service: 'niuma-remote-server',
      status: 'ok'
    })
  )
}
```

- [ ] **Step 5: Implement Fastify app**

Create `remote-server/src/app.ts`:

```ts
import Fastify from 'fastify'
import { ErrorCode } from './shared/errors.js'
import { apiFailure } from './shared/response.js'
import { registerHealthRoutes } from './modules/health/health.routes.js'

export function buildApp() {
  const app = Fastify({ logger: false })

  void registerHealthRoutes(app)

  app.setNotFoundHandler(async (_request, reply) => {
    return reply.status(404).send(apiFailure(ErrorCode.ROUTE_NOT_FOUND, '接口不存在'))
  })

  app.setErrorHandler(async (_error, _request, reply) => {
    return reply.status(500).send(apiFailure(ErrorCode.SYSTEM_ERROR, '系统异常'))
  })

  return app
}
```

- [ ] **Step 6: Implement server entrypoint**

Create `remote-server/src/server.ts`:

```ts
import { buildApp } from './app.js'
import { loadConfigFromEnv } from './config.js'

const config = loadConfigFromEnv()
const app = buildApp()

await app.listen({ host: config.bind, port: config.port })
```

- [ ] **Step 7: Run health tests**

Run:

```bash
cd remote-server
npm test -- health.test.ts
```

Expected: PASS.

- [ ] **Step 8: Run build**

Run:

```bash
cd remote-server
npm run build
```

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add remote-server/src/shared/validation.ts remote-server/src/modules/health/health.routes.ts remote-server/src/app.ts remote-server/src/server.ts remote-server/tests/health.test.ts
git commit -m "feat: 新增远程服务端健康检查接口" -m "修改内容：新增 Fastify app、健康检查路由、统一 404/500 envelope 和服务启动入口。" -m "修改原因：远程服务端需要可验证的基础 HTTP 服务和统一响应行为。"
```

## Task 5: Drizzle Schema Foundation

**Files:**
- Create: `remote-server/src/db/schema.ts`
- Create: `remote-server/src/db/client.ts`
- Create: `remote-server/drizzle.config.ts`
- Create: `remote-server/tests/schema.test.ts`

- [ ] **Step 1: Write failing schema tests**

Create `remote-server/tests/schema.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { desktopLoginSessions, devices, refreshTokens, remoteConnections, users } from '../src/db/schema.js'

describe('drizzle schema', () => {
  it('exports core tables', () => {
    expect(users).toBeDefined()
    expect(refreshTokens).toBeDefined()
    expect(devices).toBeDefined()
    expect(remoteConnections).toBeDefined()
    expect(desktopLoginSessions).toBeDefined()
  })
})
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- schema.test.ts
```

Expected: FAIL because `src/db/schema.ts` does not exist.

- [ ] **Step 3: Implement schema**

Create `remote-server/src/db/schema.ts`:

```ts
import { jsonb, pgTable, text, timestamp, uuid } from 'drizzle-orm/pg-core'

export const users = pgTable('users', {
  id: uuid('id').primaryKey().defaultRandom(),
  email: text('email').notNull().unique(),
  passwordHash: text('password_hash').notNull(),
  passwordAlgo: text('password_algo').notNull(),
  role: text('role').notNull(),
  status: text('status').notNull(),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull(),
  updatedAt: timestamp('updated_at', { withTimezone: true }).notNull(),
  passwordUpdatedAt: timestamp('password_updated_at', { withTimezone: true }).notNull()
})

export const refreshTokens = pgTable('refresh_tokens', {
  id: uuid('id').primaryKey().defaultRandom(),
  userId: uuid('user_id').notNull().references(() => users.id),
  tokenHash: text('token_hash').notNull().unique(),
  clientId: text('client_id').notNull(),
  userAgent: text('user_agent'),
  ip: text('ip'),
  expiresAt: timestamp('expires_at', { withTimezone: true }).notNull(),
  revokedAt: timestamp('revoked_at', { withTimezone: true }),
  rotatedFromId: uuid('rotated_from_id'),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull()
})

export const devices = pgTable('devices', {
  id: uuid('id').primaryKey().defaultRandom(),
  userId: uuid('user_id').notNull().references(() => users.id),
  name: text('name').notNull(),
  fingerprintHash: text('fingerprint_hash').notNull(),
  tokenHash: text('token_hash').notNull().unique(),
  status: text('status').notNull(),
  lastSeenAt: timestamp('last_seen_at', { withTimezone: true }),
  capabilityJson: jsonb('capability_json').notNull(),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull(),
  updatedAt: timestamp('updated_at', { withTimezone: true }).notNull(),
  revokedAt: timestamp('revoked_at', { withTimezone: true })
})

export const remoteConnections = pgTable('remote_connections', {
  id: uuid('id').primaryKey().defaultRandom(),
  userId: uuid('user_id').notNull().references(() => users.id),
  deviceId: uuid('device_id').notNull().references(() => devices.id),
  clientId: text('client_id').notNull(),
  status: text('status').notNull(),
  transportPreference: text('transport_preference').notNull(),
  transportSelected: text('transport_selected'),
  expiresAt: timestamp('expires_at', { withTimezone: true }).notNull(),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull(),
  connectedAt: timestamp('connected_at', { withTimezone: true }),
  closedAt: timestamp('closed_at', { withTimezone: true }),
  closeReason: text('close_reason')
})

export const desktopLoginSessions = pgTable('desktop_login_sessions', {
  id: uuid('id').primaryKey().defaultRandom(),
  requestId: text('request_id').notNull().unique(),
  pollTokenHash: text('poll_token_hash').notNull().unique(),
  desktopPublicKey: text('desktop_public_key').notNull(),
  deviceName: text('device_name').notNull(),
  fingerprintHash: text('fingerprint_hash').notNull(),
  capabilityJson: jsonb('capability_json').notNull(),
  status: text('status').notNull(),
  userId: uuid('user_id').references(() => users.id),
  deviceId: uuid('device_id').references(() => devices.id),
  encryptedResultJson: jsonb('encrypted_result_json'),
  expiresAt: timestamp('expires_at', { withTimezone: true }).notNull(),
  completedAt: timestamp('completed_at', { withTimezone: true }),
  consumedAt: timestamp('consumed_at', { withTimezone: true }),
  createdAt: timestamp('created_at', { withTimezone: true }).notNull()
})
```

- [ ] **Step 4: Implement db client**

Create `remote-server/src/db/client.ts`:

```ts
import { drizzle } from 'drizzle-orm/node-postgres'
import pg from 'pg'
import * as schema from './schema.js'

export function createDb(databaseUrl: string) {
  const pool = new pg.Pool({ connectionString: databaseUrl })
  return {
    pool,
    db: drizzle(pool, { schema })
  }
}
```

- [ ] **Step 5: Add Drizzle config**

Create `remote-server/drizzle.config.ts`:

```ts
import { defineConfig } from 'drizzle-kit'

export default defineConfig({
  schema: './src/db/schema.ts',
  out: './migrations',
  dialect: 'postgresql',
  dbCredentials: {
    url: process.env.DATABASE_URL ?? 'postgres://niuma:change-me@localhost:55432/niuma_remote'
  }
})
```

The fallback URL uses host port `55432`, not PostgreSQL default `5432`.

- [ ] **Step 6: Run schema tests and build**

Run:

```bash
cd remote-server
npm test -- schema.test.ts
npm run build
```

Expected: both PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/src/db/schema.ts remote-server/src/db/client.ts remote-server/drizzle.config.ts remote-server/tests/schema.test.ts
git commit -m "feat: 新增远程服务端数据库模型" -m "修改内容：新增用户、刷新令牌、设备、远程连接和桌面登录会话 Drizzle schema。" -m "修改原因：远程服务端需要持久化账号、设备绑定、连接记录和一次性浏览器登录绑定状态。"
```

## Task 6: Desktop Login Request Schemas

**Files:**
- Create: `remote-server/src/modules/desktopLogin/desktopLogin.schemas.ts`
- Create: `remote-server/tests/desktopLogin.schemas.test.ts`

- [ ] **Step 1: Write failing schema tests**

Create `remote-server/tests/desktopLogin.schemas.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import { desktopLoginPollSchema, desktopLoginStartSchema } from '../src/modules/desktopLogin/desktopLogin.schemas.js'

describe('desktop login schemas', () => {
  it('accepts valid start request', () => {
    const result = desktopLoginStartSchema.parse({
      device_name: 'NiuMa MacBook',
      device_fingerprint: 'a'.repeat(64),
      desktop_public_key: 'base64-public-key',
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
```

- [ ] **Step 2: Run test to verify it fails**

Run:

```bash
cd remote-server
npm test -- desktopLogin.schemas.test.ts
```

Expected: FAIL because schema file does not exist.

- [ ] **Step 3: Implement desktop login schemas**

Create `remote-server/src/modules/desktopLogin/desktopLogin.schemas.ts`:

```ts
import { z } from 'zod'

export const deviceCapabilitiesSchema = z.object({
  agent_protocol_version: z.number().int().positive(),
  rpc_protocol_version: z.number().int().positive(),
  supports_webrtc: z.boolean(),
  supports_relay: z.boolean(),
  supports_remote_control: z.boolean()
})

export const desktopLoginStartSchema = z.object({
  device_name: z.string().min(1).max(120),
  device_fingerprint: z.string().min(32).max(128),
  desktop_public_key: z.string().min(16),
  capabilities: deviceCapabilitiesSchema
})

export const desktopLoginCompleteSchema = z.object({
  request_id: z.string().min(1).max(160)
})

export const desktopLoginPollSchema = z.object({
  request_id: z.string().min(1).max(160),
  poll_token: z.string().min(32)
})

export type DesktopLoginStartInput = z.infer<typeof desktopLoginStartSchema>
export type DesktopLoginCompleteInput = z.infer<typeof desktopLoginCompleteSchema>
export type DesktopLoginPollInput = z.infer<typeof desktopLoginPollSchema>
```

- [ ] **Step 4: Run schema tests**

Run:

```bash
cd remote-server
npm test -- desktopLogin.schemas.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add remote-server/src/modules/desktopLogin/desktopLogin.schemas.ts remote-server/tests/desktopLogin.schemas.test.ts
git commit -m "feat: 新增桌面浏览器登录绑定参数模型" -m "修改内容：新增 desktop-login start、complete、poll 请求 schema 和测试。" -m "修改原因：本机 NiumaNotifier 需要通过浏览器完成账号登录和设备绑定。"
```

## Task 7: Docker Self-Hosting Skeleton

**Files:**
- Create: `remote-server/Dockerfile`
- Create: `remote-server/docker-compose.yml`
- Create: `remote-server/migrations/.gitkeep`
- Create: `remote-server/src/db/migrate.ts`

- [ ] **Step 1: Implement migration runner**

Create `remote-server/src/db/migrate.ts`:

```ts
import { migrate } from 'drizzle-orm/node-postgres/migrator'
import { loadConfigFromEnv } from '../config.js'
import { createDb } from './client.js'

const config = loadConfigFromEnv()
const { db, pool } = createDb(config.databaseUrl)

try {
  await migrate(db, { migrationsFolder: './migrations' })
} finally {
  await pool.end()
}
```

- [ ] **Step 2: Add Dockerfile**

Create `remote-server/Dockerfile`:

```dockerfile
FROM node:22-alpine AS deps
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci

FROM node:22-alpine AS build
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY package.json package-lock.json tsconfig.json ./
COPY src ./src
RUN npm run build

FROM node:22-alpine AS runner
WORKDIR /app
ENV NODE_ENV=production
COPY package.json package-lock.json ./
RUN npm ci --omit=dev
COPY --from=build /app/dist ./dist
COPY migrations ./migrations
EXPOSE 27880
CMD ["sh", "-c", "node dist/db/migrate.js && node dist/server.js"]
```

- [ ] **Step 3: Keep migrations directory in git**

Create `remote-server/migrations/.gitkeep`:

```text

```

The empty file ensures `COPY migrations ./migrations` works before the first generated migration exists.

- [ ] **Step 4: Add Docker Compose**

Create `remote-server/docker-compose.yml`:

```yaml
services:
  remote-server:
    build: .
    ports:
      - "27880:27880"
    env_file:
      - .env
    depends_on:
      - postgres
      - redis

  postgres:
    image: postgres:16-alpine
    environment:
      POSTGRES_DB: niuma_remote
      POSTGRES_USER: niuma
      POSTGRES_PASSWORD: change-me
    volumes:
      - postgres_data:/var/lib/postgresql/data

  redis:
    image: redis:7-alpine
    command: ["redis-server", "--appendonly", "yes"]
    volumes:
      - redis_data:/data

  coturn:
    image: coturn/coturn:latest
    profiles:
      - turn
    ports:
      - "13478:3478/udp"
      - "13478:3478/tcp"
    command:
      - --listening-port=3478
      - --fingerprint
      - --lt-cred-mech
      - --realm=niuma-remote

volumes:
  postgres_data:
  redis_data:
```

PostgreSQL and Redis have no host `ports:` mappings. The only host-facing default service port in this file is intentionally avoided; the API uses `27880`, TURN uses `13478`.

- [ ] **Step 5: Run default-port scan**

Run:

```bash
rg -n "80:80|443:443|5432:5432|6379:6379|8080:8080|3000:3000|5173:5173" remote-server
```

Expected: no output.

- [ ] **Step 6: Run build**

Run:

```bash
cd remote-server
npm run build
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add remote-server/Dockerfile remote-server/docker-compose.yml remote-server/migrations/.gitkeep remote-server/src/db/migrate.ts
git commit -m "feat: 新增远程服务端自托管部署骨架" -m "修改内容：新增 Dockerfile、Docker Compose 和数据库迁移启动入口，使用 27880 和 13478 作为宿主端口。" -m "修改原因：远程服务端需要能通过 Docker Compose 自托管部署，并避免占用常见默认端口。"
```

## Task 8: Final Verification For Milestone

**Files:**
- Verify all files created in Tasks 1-7.

- [ ] **Step 1: Run full remote-server checks**

Run:

```bash
cd remote-server
npm run check
```

Expected: TypeScript build passes and Vitest tests pass.

- [ ] **Step 2: Run default-port scan**

Run:

```bash
rg -n "80:80|443:443|5432:5432|6379:6379|8080:8080|3000:3000|5173:5173" remote-server
```

Expected: no output.

- [ ] **Step 3: Inspect git status**

Run:

```bash
git status --short
```

Expected: no uncommitted changes.

- [ ] **Step 4: Record milestone result**

Add a short note to the implementation issue or PR description:

```text
Remote server foundation complete:
- TypeScript/Fastify skeleton
- Standard API envelope
- Config loader with non-default port policy
- Health endpoint
- Drizzle schema foundation
- Desktop-login request schemas
- Docker Compose skeleton

Verification:
- cd remote-server && npm run check
- rg default host port scan
```

Do not mark remote access complete after this milestone. This milestone only establishes the server foundation.

## Self-Review

Spec coverage in this plan:

- Remote server technical stack: covered by Tasks 1, 5, and 7.
- Unified API envelope: covered by Task 2 and Task 4.
- Non-default host ports: covered by Task 3 and Task 7.
- PostgreSQL schema foundation: covered by Task 5.
- Desktop browser-login binding request contract: covered by Task 6.
- Docker self-hosting skeleton: covered by Task 7.

Explicitly outside this plan and requiring separate implementation plans:

- Auth service behavior with password hashing and refresh token rotation.
- Desktop-login service persistence and encrypted result generation.
- `/ws/device`, `/ws/client`, and `/ws/relay`.
- Remote Web console UI.
- Local Rust RemoteAgent.
- WebRTC, relay frame forwarding, and E2EE RPC.
