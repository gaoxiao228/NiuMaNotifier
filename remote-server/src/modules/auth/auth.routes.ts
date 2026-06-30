import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { apiFailure, apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { createAuthRepository } from './auth.repository.js'
import { requireAuth } from './auth.middleware.js'
import { authLoginSchema, authLogoutSchema, authRefreshSchema, authRegisterSchema } from './auth.schemas.js'
import { createAuthService } from './auth.service.js'

function headerClientId(value: string | string[] | undefined): string {
  if (Array.isArray(value)) return value[0] ?? 'web'
  return value ?? 'web'
}

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

    const result = await service.login({
      ...parsed.data,
      clientId: headerClientId(request.headers['user-agent'])
    })
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.post('/api/v1/admin/auth/login', async (request) => {
    const parsed = parseBody(authLoginSchema, request.body)
    if (!parsed.ok) return parsed.response

    const result = await service.loginAdmin({
      ...parsed.data,
      clientId: headerClientId(request.headers['user-agent'])
    })
    return result.ok ? apiSuccess(result.data) : apiFailure(result.code, result.message)
  })

  app.post('/api/v1/auth/refresh', async (request) => {
    const parsed = parseBody(authRefreshSchema, request.body)
    if (!parsed.ok) return parsed.response

    const result = await service.refresh({
      refreshToken: parsed.data.refresh_token,
      clientId: headerClientId(request.headers['user-agent'])
    })
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
