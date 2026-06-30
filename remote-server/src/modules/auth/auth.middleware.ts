import type { FastifyRequest } from 'fastify'
import { ErrorCode } from '../../shared/errors.js'
import { apiFailure } from '../../shared/response.js'
import { verifyAccessToken } from './token.service.js'

export type AuthContext = {
  userId: string
  sessionId: string
  role: 'admin' | 'user'
}

export async function requireAuth(
  request: FastifyRequest,
  publicKeyPem: string
): Promise<{ ok: true; auth: AuthContext } | { ok: false; response: unknown }> {
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
