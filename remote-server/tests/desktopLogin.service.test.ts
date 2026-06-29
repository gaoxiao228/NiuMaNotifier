import { compactDecrypt, exportJWK, generateKeyPair } from 'jose'
import { describe, expect, it } from 'vitest'
import {
  createDesktopLoginService,
  type DesktopLoginRepository
} from '../src/modules/desktopLogin/desktopLogin.service.js'
import { ErrorCode } from '../src/shared/errors.js'

function createRepo(options?: {
  beforeDeviceBindingTransaction?: (repo: DesktopLoginRepository) => Promise<void> | void
  beforeCompleteSession?: (
    repo: DesktopLoginRepository,
    requestId: string
  ) => Promise<void> | void
}): DesktopLoginRepository {
  const sessions = new Map<string, any>()
  const devices = new Map<string, any>()

  const repo: DesktopLoginRepository = {
    async createSession(input) {
      sessions.set(input.requestId, { id: `dls_${sessions.size + 1}`, ...input })
      return sessions.get(input.requestId)
    },
    async findSessionByRequestId(requestId) {
      return sessions.get(requestId) ?? null
    },
    async completeSession(requestId, input) {
      await options?.beforeCompleteSession?.(repo, requestId)
      const session = sessions.get(requestId)
      if (session?.status !== 'pending') return false
      Object.assign(session, input)
      return true
    },
    async consumeSession(requestId) {
      sessions.get(requestId).status = 'consumed'
      sessions.get(requestId).consumedAt = new Date()
    },
    async consumeOtherSessionsForDevice(input) {
      for (const session of sessions.values()) {
        // 新绑定完成后，旧的 pending/completed 会话不能再返回已轮换的设备 token。
        if (
          session.fingerprintHash === input.fingerprintHash &&
          session.requestId !== input.requestId &&
          ((session.status === 'pending' && session.createdAt < input.createdBefore) ||
            (session.status === 'completed' && session.userId === input.userId))
        ) {
          session.status = 'consumed'
          session.consumedAt = input.consumedAt
        }
      }
    },
    async upsertDevice(input) {
      const existing = [...devices.values()].find(
        (device) =>
          device.userId === input.userId &&
          device.fingerprintHash === input.fingerprintHash &&
          device.status === 'active'
      )
      if (existing) {
        Object.assign(existing, input)
        return existing
      }
      const device = { id: `dev_${devices.size + 1}`, ...input }
      devices.set(device.id, device)
      return device
    },
    async runDeviceBindingTransaction(_fingerprintHash, action) {
      // 测试用钩子模拟真实事务拿锁后，当前会话已被另一条 complete 流程消费。
      await options?.beforeDeviceBindingTransaction?.(repo)
      return action(repo)
    }
  }

  return repo
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

function createMutableClock(start = new Date('2026-06-29T00:00:00.000Z')) {
  let current = start
  return {
    now: () => current,
    advance(milliseconds: number) {
      current = new Date(current.getTime() + milliseconds)
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

  it('reuses active device for the same user and fingerprint', async () => {
    const first = await validStartInput()
    const second = await validStartInput()
    second.input.device_fingerprint = first.input.device_fingerprint
    const repo = createRepo()
    const service = createDesktopLoginService({
      repo,
      config: {
        publicUrl: 'https://remote.example.com',
        tokenPepper: 'pepper',
        desktopLoginTtlSeconds: 600
      }
    })

    const startOne = await service.start(first.input)
    if (!startOne.ok) throw new Error('start failed')

    await service.complete({
      requestId: startOne.data.request_id,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })
    const startTwo = await service.start(second.input)
    if (!startTwo.ok) throw new Error('start failed')
    await service.complete({
      requestId: startTwo.data.request_id,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })

    const completedTwo = await service.poll({
      request_id: startTwo.data.request_id,
      poll_token: startTwo.data.poll_token
    })
    expect(completedTwo.ok).toBe(true)
    if (!completedTwo.ok) throw new Error('poll failed')

    const completedOne = await service.poll({
      request_id: startOne.data.request_id,
      poll_token: startOne.data.poll_token
    })
    expect(completedOne).toEqual({
      ok: false,
      code: ErrorCode.DESKTOP_LOGIN_CONSUMED,
      message: '桌面登录会话已被消费'
    })

    const deviceTwo = JSON.parse(
      new TextDecoder().decode(
        (await compactDecrypt(completedTwo.data.encrypted_result.jwe, second.privateKey)).plaintext
      )
    ).device

    expect(deviceTwo.id).toBe('dev_1')
  })

  it('consumes older pending session for the same fingerprint when latest session completes', async () => {
    const first = await validStartInput()
    const second = await validStartInput()
    second.input.device_fingerprint = first.input.device_fingerprint
    const clock = createMutableClock()
    const service = createDesktopLoginService({
      repo: createRepo(),
      config: {
        publicUrl: 'https://remote.example.com',
        tokenPepper: 'pepper',
        desktopLoginTtlSeconds: 600
      },
      clock
    })

    const startOne = await service.start(first.input)
    clock.advance(1000)
    const startTwo = await service.start(second.input)
    if (!startOne.ok || !startTwo.ok) throw new Error('start failed')

    await service.complete({
      requestId: startTwo.data.request_id,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })

    const superseded = await service.poll({
      request_id: startOne.data.request_id,
      poll_token: startOne.data.poll_token
    })

    expect(superseded).toEqual({
      ok: false,
      code: ErrorCode.DESKTOP_LOGIN_CONSUMED,
      message: '桌面登录会话已被消费'
    })
  })

  it('keeps newer pending session when an older same-fingerprint session completes', async () => {
    const first = await validStartInput()
    const second = await validStartInput()
    second.input.device_fingerprint = first.input.device_fingerprint
    const clock = createMutableClock()
    const service = createDesktopLoginService({
      repo: createRepo(),
      config: {
        publicUrl: 'https://remote.example.com',
        tokenPepper: 'pepper',
        desktopLoginTtlSeconds: 600
      },
      clock
    })

    const startOne = await service.start(first.input)
    clock.advance(1000)
    const startTwo = await service.start(second.input)
    if (!startOne.ok || !startTwo.ok) throw new Error('start failed')

    await service.complete({
      requestId: startOne.data.request_id,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })

    const newerStillPending = await service.poll({
      request_id: startTwo.data.request_id,
      poll_token: startTwo.data.poll_token
    })

    expect(newerStillPending).toMatchObject({
      ok: false,
      code: ErrorCode.DESKTOP_LOGIN_PENDING
    })
  })

  it('returns consumed when pending completion write is skipped by a race', async () => {
    const { input } = await validStartInput()
    const repo = createRepo({
      beforeCompleteSession: async (transactionRepo, requestId) => {
        await transactionRepo.consumeSession(requestId)
      }
    })
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

    const result = await service.complete({
      requestId: start.data.request_id,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })

    expect(result).toEqual({
      ok: false,
      code: ErrorCode.DESKTOP_LOGIN_CONSUMED,
      message: '桌面登录会话已被消费'
    })
  })

  it('returns consumed when session is consumed inside device binding transaction', async () => {
    const { input } = await validStartInput()
    let requestId = ''
    const repo = createRepo({
      beforeDeviceBindingTransaction: async (transactionRepo) => {
        await transactionRepo.completeSession(requestId, {
          status: 'consumed',
          consumedAt: new Date()
        })
      }
    })
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
    requestId = start.data.request_id

    const result = await service.complete({
      requestId,
      user: { id: 'usr_1', email: 'user@example.com', role: 'user' }
    })

    expect(result).toEqual({
      ok: false,
      code: ErrorCode.DESKTOP_LOGIN_CONSUMED,
      message: '桌面登录会话已被消费'
    })
  })
})
