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
