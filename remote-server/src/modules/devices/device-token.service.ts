import { createHash } from '../../shared/crypto.js'
import { ErrorCode } from '../../shared/errors.js'

export type AuthenticatedDevice = {
  id: string
  userId: string
  name: string
  status: string
  revokedAt: Date | null
  capabilityJson: unknown
}

export type DeviceTokenRepository = {
  findActiveDeviceByTokenHash(tokenHash: string): Promise<AuthenticatedDevice | null>
  updateLastSeen(deviceId: string, lastSeenAt: Date, capabilities: unknown): Promise<void>
}

export function createDeviceTokenService(options: {
  repo: DeviceTokenRepository
  tokenPepper: string
}) {
  return {
    async authenticate(authorizationHeader: string | undefined) {
      if (!authorizationHeader?.startsWith('Device ')) {
        return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_INVALID, message: '设备 token 无效' }
      }

      const token = authorizationHeader.slice('Device '.length).trim()
      if (!token) {
        return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_INVALID, message: '设备 token 无效' }
      }

      const device = await options.repo.findActiveDeviceByTokenHash(createHash(token, options.tokenPepper))
      if (!device) {
        return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_INVALID, message: '设备 token 无效' }
      }
      if (device.status !== 'active' || device.revokedAt) {
        return { ok: false as const, code: ErrorCode.DEVICE_TOKEN_REVOKED, message: '设备 token 已吊销' }
      }

      return { ok: true as const, device }
    }
  }
}
