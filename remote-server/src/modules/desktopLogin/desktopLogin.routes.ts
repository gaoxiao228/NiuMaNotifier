import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../../config.js'
import { createDb } from '../../db/client.js'
import { ErrorCode } from '../../shared/errors.js'
import { apiFailure, apiSuccess } from '../../shared/response.js'
import { parseBody } from '../../shared/validation.js'
import { requireAuth } from '../auth/auth.middleware.js'
import { createAuthRepository } from '../auth/auth.repository.js'
import {
  desktopLoginCompleteSchema,
  desktopLoginPollSchema,
  desktopLoginStartSchema
} from './desktopLogin.schemas.js'
import { createDesktopLoginRepository } from './desktopLogin.repository.js'
import { createDesktopLoginService } from './desktopLogin.service.js'

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
    return apiSuccess(result.data)
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
