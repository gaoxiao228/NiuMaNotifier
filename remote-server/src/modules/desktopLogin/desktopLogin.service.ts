import { createHash, createRandomToken } from '../../shared/crypto.js'
import { ErrorCode, type ErrorCodeValue } from '../../shared/errors.js'
import { createPublicId } from '../../shared/id.js'
import { addSeconds, secondsUntil, systemClock, type Clock } from '../../shared/time.js'
import {
  encryptDesktopLoginResult,
  type DesktopLoginEncryptedResult
} from './desktopLogin.crypto.js'
import type { DesktopLoginStartInput } from './desktopLogin.schemas.js'

export type DesktopLoginSession = {
  id: string
  requestId: string
  pollTokenHash: string
  desktopPublicKey: string
  deviceIdentityPublicKey: string
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
  consumeOtherSessionsForDevice(input: {
    userId: string
    fingerprintHash: string
    requestId: string
    consumedAt: Date
  }): Promise<void>
  upsertDevice(input: {
    userId: string
    name: string
    fingerprintHash: string
    tokenHash: string
    identityPublicKeyJson: unknown
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

function failure(
  code: ErrorCodeValue,
  message: string,
  data?: Record<string, unknown>
): DesktopLoginFailure {
  return { ok: false, code, message, ...(data ? { data } : {}) }
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
        deviceIdentityPublicKey: input.device_identity_public_key,
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

    async complete(input: {
      requestId: string
      user: { id: string; email: string; role: 'admin' | 'user' }
    }) {
      const session = await options.repo.findSessionByRequestId(input.requestId)
      if (!session) return failure(ErrorCode.DESKTOP_LOGIN_NOT_FOUND, '桌面登录会话不存在')
      if (isExpired(session)) return failure(ErrorCode.DESKTOP_LOGIN_EXPIRED, '桌面登录会话已过期')
      if (session.status !== 'pending') {
        return failure(ErrorCode.DESKTOP_LOGIN_CONSUMED, '桌面登录会话已被消费')
      }

      const now = clock.now()
      const deviceToken = createRandomToken('dvt')
      const device = await options.repo.upsertDevice({
        userId: input.user.id,
        name: session.deviceName,
        fingerprintHash: session.fingerprintHash,
        tokenHash: createHash(deviceToken, options.config.tokenPepper),
        identityPublicKeyJson: JSON.parse(session.deviceIdentityPublicKey),
        status: 'active',
        capabilityJson: session.capabilityJson,
        createdAt: now,
        updatedAt: now,
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
      await options.repo.consumeOtherSessionsForDevice({
        userId: input.user.id,
        fingerprintHash: session.fingerprintHash,
        requestId: input.requestId,
        consumedAt: clock.now()
      })

      return { ok: true as const, data: {} }
    },

    async poll(input: { request_id: string; poll_token: string }) {
      const session = await options.repo.findSessionByRequestId(input.request_id)
      if (!session) return failure(ErrorCode.DESKTOP_LOGIN_NOT_FOUND, '桌面登录会话不存在')
      if (session.pollTokenHash !== createHash(input.poll_token, options.config.tokenPepper)) {
        return failure(ErrorCode.DESKTOP_LOGIN_POLL_TOKEN_INVALID, '桌面登录轮询 token 无效')
      }
      if (isExpired(session)) return failure(ErrorCode.DESKTOP_LOGIN_EXPIRED, '桌面登录会话已过期')
      if (session.status === 'pending') {
        return failure(ErrorCode.DESKTOP_LOGIN_PENDING, '桌面登录会话尚未完成', {
          status: 'pending',
          expires_in: secondsUntil(clock.now(), session.expiresAt)
        })
      }
      if (session.status === 'consumed') {
        return failure(ErrorCode.DESKTOP_LOGIN_CONSUMED, '桌面登录会话已被消费')
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
