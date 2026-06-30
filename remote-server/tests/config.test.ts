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
    expect(config.corsOrigins).toEqual(['http://127.0.0.1:27883'])
    expect(config.registrationMode).toBe('admin_invite')
  })

  it('parses explicit remote client cors origins', () => {
    const config = loadConfigFromEnv({
      REMOTE_SERVER_CORS_ORIGINS: 'http://127.0.0.1:27883, https://client.example.com ',
      DATABASE_URL: 'postgres://niuma:pw@postgres:5432/niuma_remote',
      REDIS_URL: 'redis://redis:6379',
      JWT_PRIVATE_KEY: 'private',
      JWT_PUBLIC_KEY: 'public',
      TOKEN_PEPPER: 'pepper'
    })

    expect(config.corsOrigins).toEqual(['http://127.0.0.1:27883', 'https://client.example.com'])
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

  it('accepts base64 encoded jwt keys for docker env files', () => {
    const privatePem = '-----BEGIN PRIVATE KEY-----\nprivate\n-----END PRIVATE KEY-----\n'
    const publicPem = '-----BEGIN PUBLIC KEY-----\npublic\n-----END PUBLIC KEY-----\n'
    const config = loadConfigFromEnv({
      DATABASE_URL: 'postgres://niuma:pw@postgres:5432/niuma_remote',
      REDIS_URL: 'redis://redis:6379',
      JWT_PRIVATE_KEY_BASE64: Buffer.from(privatePem).toString('base64'),
      JWT_PUBLIC_KEY_BASE64: Buffer.from(publicPem).toString('base64'),
      TOKEN_PEPPER: 'pepper'
    })

    expect(config.jwtPrivateKey).toBe(privatePem)
    expect(config.jwtPublicKey).toBe(publicPem)
  })

  it('wraps base64 der jwt keys as pem for docker env files', () => {
    const config = loadConfigFromEnv({
      DATABASE_URL: 'postgres://niuma:pw@postgres:5432/niuma_remote',
      REDIS_URL: 'redis://redis:6379',
      JWT_PRIVATE_KEY_BASE64: 'cHJpdmF0ZQ==',
      JWT_PUBLIC_KEY_BASE64: 'cHVibGlj',
      TOKEN_PEPPER: 'pepper'
    })

    expect(config.jwtPrivateKey).toContain('-----BEGIN PRIVATE KEY-----')
    expect(config.jwtPublicKey).toContain('-----BEGIN PUBLIC KEY-----')
  })
})
