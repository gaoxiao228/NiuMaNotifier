import { z } from 'zod'

// 这里校验的是宿主机暴露端口，避免和常见框架/基础设施默认端口冲突。
const blockedHostPorts = new Set([80, 443, 3000, 5000, 5173, 8000, 8080, 5432, 6379])

const envSchema = z.object({
  REMOTE_SERVER_PUBLIC_URL: z.string().url().default('http://127.0.0.1:27880'),
  REMOTE_SERVER_BIND: z.string().default('0.0.0.0'),
  REMOTE_SERVER_PORT: z.coerce.number().int().positive().default(27880),
  REMOTE_SERVER_CORS_ORIGINS: z.string().default('http://127.0.0.1:27883,http://localhost:27883'),
  DATABASE_URL: z.string().min(1),
  REDIS_URL: z.string().min(1),
  JWT_PRIVATE_KEY: z.string().min(1).optional(),
  JWT_PUBLIC_KEY: z.string().min(1).optional(),
  JWT_PRIVATE_KEY_BASE64: z.string().min(1).optional(),
  JWT_PUBLIC_KEY_BASE64: z.string().min(1).optional(),
  TOKEN_PEPPER: z.string().min(1),
  ACCESS_TOKEN_TTL_SECONDS: z.coerce.number().int().positive().default(900),
  REFRESH_TOKEN_TTL_DAYS: z.coerce.number().int().positive().default(30),
  CONNECTION_TOKEN_TTL_SECONDS: z.coerce.number().int().positive().default(120),
  DEVICE_PRESENCE_TTL_SECONDS: z.coerce.number().int().positive().default(90),
  DEVICE_HEARTBEAT_TIMEOUT_SECONDS: z.coerce.number().int().positive().default(45),
  REGISTRATION_MODE: z.enum(['open', 'admin_invite', 'disabled']).default('admin_invite'),
  BOOTSTRAP_ADMIN_EMAIL: z.string().email().optional(),
  BOOTSTRAP_ADMIN_PASSWORD: z.string().min(8).max(128).optional(),
  TURN_ENABLED: z.coerce.boolean().default(false),
  TURN_URLS: z.string().default(''),
  TURN_USERNAME: z.string().default(''),
  TURN_CREDENTIAL: z.string().default('')
})

export type RemoteServerConfig = ReturnType<typeof loadConfigFromEnv>

function wrapPemBody(body: string, label: 'PRIVATE KEY' | 'PUBLIC KEY') {
  const lines = body.match(/.{1,64}/g)?.join('\n') ?? body
  return `-----BEGIN ${label}-----\n${lines}\n-----END ${label}-----`
}

function decodeBase64Env(value: string, name: string, label: 'PRIVATE KEY' | 'PUBLIC KEY') {
  try {
    const decoded = Buffer.from(value, 'base64').toString('utf8')
    return decoded.includes('-----BEGIN ') ? decoded : wrapPemBody(value, label)
  } catch {
    throw new Error(`${name} 不是有效的 base64 字符串`)
  }
}

function requiredKey(
  pem: string | undefined,
  base64: string | undefined,
  name: string,
  label: 'PRIVATE KEY' | 'PUBLIC KEY'
) {
  if (pem) return pem
  if (base64) return decodeBase64Env(base64, `${name}_BASE64`, label)
  throw new Error(`${name} 或 ${name}_BASE64 必须配置`)
}

export function loadConfigFromEnv(env: NodeJS.ProcessEnv = process.env) {
  const parsed = envSchema.parse(env)

  if (blockedHostPorts.has(parsed.REMOTE_SERVER_PORT)) {
    throw new Error('REMOTE_SERVER_PORT 不能使用常见默认端口')
  }

  return {
    publicUrl: parsed.REMOTE_SERVER_PUBLIC_URL,
    bind: parsed.REMOTE_SERVER_BIND,
    port: parsed.REMOTE_SERVER_PORT,
    corsOrigins: parsed.REMOTE_SERVER_CORS_ORIGINS.split(',').map((item) => item.trim()).filter(Boolean),
    databaseUrl: parsed.DATABASE_URL,
    redisUrl: parsed.REDIS_URL,
    jwtPrivateKey: requiredKey(
      parsed.JWT_PRIVATE_KEY,
      parsed.JWT_PRIVATE_KEY_BASE64,
      'JWT_PRIVATE_KEY',
      'PRIVATE KEY'
    ),
    jwtPublicKey: requiredKey(
      parsed.JWT_PUBLIC_KEY,
      parsed.JWT_PUBLIC_KEY_BASE64,
      'JWT_PUBLIC_KEY',
      'PUBLIC KEY'
    ),
    tokenPepper: parsed.TOKEN_PEPPER,
    accessTokenTtlSeconds: parsed.ACCESS_TOKEN_TTL_SECONDS,
    refreshTokenTtlDays: parsed.REFRESH_TOKEN_TTL_DAYS,
    connectionTokenTtlSeconds: parsed.CONNECTION_TOKEN_TTL_SECONDS,
    devicePresenceTtlSeconds: parsed.DEVICE_PRESENCE_TTL_SECONDS,
    deviceHeartbeatTimeoutSeconds: parsed.DEVICE_HEARTBEAT_TIMEOUT_SECONDS,
    registrationMode: parsed.REGISTRATION_MODE,
    bootstrapAdmin: {
      email: parsed.BOOTSTRAP_ADMIN_EMAIL,
      password: parsed.BOOTSTRAP_ADMIN_PASSWORD
    },
    turn: {
      enabled: parsed.TURN_ENABLED,
      urls: parsed.TURN_URLS.split(',').map((item) => item.trim()).filter(Boolean),
      username: parsed.TURN_USERNAME,
      credential: parsed.TURN_CREDENTIAL
    }
  }
}
