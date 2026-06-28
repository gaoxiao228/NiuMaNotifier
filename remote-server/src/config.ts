import { z } from 'zod'

// 这里校验的是宿主机暴露端口，避免和常见框架/基础设施默认端口冲突。
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
  DEVICE_PRESENCE_TTL_SECONDS: z.coerce.number().int().positive().default(90),
  DEVICE_HEARTBEAT_TIMEOUT_SECONDS: z.coerce.number().int().positive().default(45),
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
    devicePresenceTtlSeconds: parsed.DEVICE_PRESENCE_TTL_SECONDS,
    deviceHeartbeatTimeoutSeconds: parsed.DEVICE_HEARTBEAT_TIMEOUT_SECONDS,
    registrationMode: parsed.REGISTRATION_MODE,
    turn: {
      enabled: parsed.TURN_ENABLED,
      urls: parsed.TURN_URLS.split(',').map((item) => item.trim()).filter(Boolean),
      username: parsed.TURN_USERNAME,
      credential: parsed.TURN_CREDENTIAL
    }
  }
}
