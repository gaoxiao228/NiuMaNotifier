import { describe, expect, it } from 'vitest'
import {
  createDeviceTokenService,
  type DeviceTokenRepository
} from '../src/modules/devices/device-token.service.js'
import { createHash } from '../src/shared/crypto.js'
import { ErrorCode } from '../src/shared/errors.js'

function createRepo(): DeviceTokenRepository {
  const tokenHash = createHash('dvt_valid', 'pepper')

  return {
    async findActiveDeviceByTokenHash(inputHash) {
      if (inputHash !== tokenHash) return null

      return {
        id: 'dev_1',
        userId: 'usr_1',
        name: 'NiuMa MacBook',
        status: 'active',
        revokedAt: null,
        capabilityJson: {}
      }
    },
    async updateLastSeen() {}
  }
}

describe('device token service', () => {
  it('accepts valid active device token', async () => {
    const service = createDeviceTokenService({ repo: createRepo(), tokenPepper: 'pepper' })

    const result = await service.authenticate('Device dvt_valid')

    expect(result.ok).toBe(true)
    if (result.ok) expect(result.device.id).toBe('dev_1')
  })

  it('rejects bearer access token for device socket', async () => {
    const service = createDeviceTokenService({ repo: createRepo(), tokenPepper: 'pepper' })

    await expect(service.authenticate('Bearer access_token')).resolves.toEqual({
      ok: false,
      code: ErrorCode.DEVICE_TOKEN_INVALID,
      message: '设备 token 无效'
    })
  })
})
