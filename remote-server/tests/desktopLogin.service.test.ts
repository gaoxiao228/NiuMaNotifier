import { compactDecrypt, exportJWK, generateKeyPair } from 'jose'
import { describe, expect, it } from 'vitest'
import {
  createDesktopLoginService,
  type DesktopLoginRepository
} from '../src/modules/desktopLogin/desktopLogin.service.js'
import { ErrorCode } from '../src/shared/errors.js'

function createRepo(): DesktopLoginRepository {
  const sessions = new Map<string, any>()
  const devices = new Map<string, any>()

  return {
    async createSession(input) {
      sessions.set(input.requestId, { id: `dls_${sessions.size + 1}`, ...input })
      return sessions.get(input.requestId)
    },
    async findSessionByRequestId(requestId) {
      return sessions.get(requestId) ?? null
    },
    async completeSession(requestId, input) {
      Object.assign(sessions.get(requestId), input)
    },
    async consumeSession(requestId) {
      sessions.get(requestId).status = 'consumed'
      sessions.get(requestId).consumedAt = new Date()
    },
    async findActiveDeviceByFingerprint(userId, fingerprintHash) {
      return (
        [...devices.values()].find(
          (device) =>
            device.userId === userId &&
            device.fingerprintHash === fingerprintHash &&
            device.status === 'active'
        ) ?? null
      )
    },
    async upsertDevice(input) {
      const existing = [...devices.values()].find(
        (device) => device.userId === input.userId && device.fingerprintHash === input.fingerprintHash
      )
      if (existing) {
        Object.assign(existing, input)
        return existing
      }
      const device = { id: `dev_${devices.size + 1}`, ...input }
      devices.set(device.id, device)
      return device
    }
  }
}

const validStartInput = async () => {
  const { publicKey, privateKey } = await generateKeyPair('ECDH-ES')
  const identity = {
    kty: 'EC',
    crv: 'P-256',
    x: 'x-coordinate',
    y: 'y-coordinate'
  }

  return {
    privateKey,
    input: {
      device_name: 'NiuMa MacBook',
      device_fingerprint: 'fingerprint-1234567890-abcdef',
      desktop_public_key: JSON.stringify(await exportJWK(publicKey)),
      device_identity_public_key: JSON.stringify(identity),
      capabilities: {
        agent_protocol_version: 1,
        rpc_protocol_version: 1,
        supports_webrtc: true,
        supports_relay: true,
        supports_remote_control: true
      }
    }
  }
}

describe('desktop login service', () => {
  it('starts session without leaking poll token in login url', async () => {
    const { input } = await validStartInput()
    const service = createDesktopLoginService({
      repo: createRepo(),
      config: {
        publicUrl: 'https://remote.example.com',
        tokenPepper: 'pepper',
        desktopLoginTtlSeconds: 600
      }
    })

    const result = await service.start(input)

    expect(result.ok).toBe(true)
    if (!result.ok) throw new Error('start failed')
    expect(result.data.login_url).toContain(`request_id=${result.data.request_id}`)
    expect(result.data.login_url).not.toContain(result.data.poll_token)
  })

  it('returns pending before complete and consumed after completed poll', async () => {
    const { input, privateKey } = await validStartInput()
    const repo = createRepo()
    const service = createDesktopLoginService({
      repo,
      config: {
        publicUrl: 'https://remote.example.com',
        tokenPepper: 'pepper',
        desktopLoginTtlSeconds: 600
      }
    })

    const start = await service.start(input)
    if (!start.ok) throw new Error('start failed')

    const pending = await service.poll({
      request_id: start.data.request_id,
      poll_token: start.data.poll_token
    })
    expect(pending).toMatchObject({ ok: false, code: ErrorCode.DESKTOP_LOGIN_PENDING })

    await service.complete({
      requestId: start.data.request_id,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })
    const completed = await service.poll({
      request_id: start.data.request_id,
      poll_token: start.data.poll_token
    })
    expect(completed.ok).toBe(true)
    if (!completed.ok) throw new Error('poll failed')
    const decrypted = await compactDecrypt(completed.data.encrypted_result.jwe, privateKey)
    expect(JSON.parse(new TextDecoder().decode(decrypted.plaintext)).device_token).toMatch(/^dvt_/)

    const consumed = await service.poll({
      request_id: start.data.request_id,
      poll_token: start.data.poll_token
    })
    expect(consumed).toEqual({
      ok: false,
      code: ErrorCode.DESKTOP_LOGIN_CONSUMED,
      message: '桌面登录会话已被消费'
    })
  })
})
