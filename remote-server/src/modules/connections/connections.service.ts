import { ErrorCode, type ErrorCodeValue } from '../../shared/errors.js'
import { createPublicId } from '../../shared/id.js'
import { addSeconds, systemClock, type Clock } from '../../shared/time.js'
import { createConnectionTokenService } from './connection-token.service.js'

export type ConnectionsRepository = {
  findDeviceForUser(
    userId: string,
    deviceId: string
  ): Promise<{ id: string; userId: string; name: string; status: string } | null>
  createConnection(input: {
    id: string
    userId: string
    deviceId: string
    clientId: string
    status: string
    transportPreference: string
    transportSelected: string | null
    expiresAt: Date
    createdAt: Date
    connectedAt: Date | null
    closedAt: Date | null
    closeReason: string | null
  }): Promise<{ id: string }>
}

export type ConnectionPresenceReader = {
  getPresence(deviceId: string): Promise<unknown | null>
}

export type ConnectionStateWriter = {
  setPending(input: {
    connectionId: string
    userId: string
    deviceId: string
    clientId: string
    tokenHash: string
    status: 'pending' | 'signaling'
    createdAt: string
    expiresAt: string
  }): Promise<void>
}

export type ConnectionFailure = {
  ok: false
  code: ErrorCodeValue
  message: string
}

function toWebSocketUrl(publicUrl: string, path: string) {
  const url = new URL(publicUrl)
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
  url.pathname = path
  url.search = ''
  return url.toString()
}

export function createConnectionsService(options: {
  repo: ConnectionsRepository
  presence: ConnectionPresenceReader
  state: ConnectionStateWriter
  tokenPepper: string
  publicUrl: string
  ttlSeconds: number
  clock?: Clock
}) {
  const clock = options.clock ?? systemClock
  const tokenService = createConnectionTokenService({ tokenPepper: options.tokenPepper })

  return {
    async create(input: {
      userId: string
      deviceId: string
      clientId: string
      transportPreference: 'webrtc_first' | 'relay_first' | 'relay_only'
    }) {
      const device = await options.repo.findDeviceForUser(input.userId, input.deviceId)
      if (!device) return { ok: false as const, code: ErrorCode.DEVICE_NOT_FOUND, message: '设备不存在' }

      const presence = await options.presence.getPresence(input.deviceId)
      if (!presence) return { ok: false as const, code: ErrorCode.DEVICE_OFFLINE, message: '设备离线' }

      const now = clock.now()
      const expiresAt = addSeconds(now, options.ttlSeconds)
      const connectionId = createPublicId('conn')
      const issued = tokenService.issue()

      await options.repo.createConnection({
        id: connectionId,
        userId: input.userId,
        deviceId: input.deviceId,
        clientId: input.clientId,
        status: 'signaling',
        transportPreference: input.transportPreference,
        transportSelected: null,
        expiresAt,
        createdAt: now,
        connectedAt: null,
        closedAt: null,
        closeReason: null
      })

      await options.state.setPending({
        connectionId,
        userId: input.userId,
        deviceId: input.deviceId,
        clientId: input.clientId,
        tokenHash: issued.tokenHash,
        status: 'signaling',
        createdAt: now.toISOString(),
        expiresAt: expiresAt.toISOString()
      })

      return {
        ok: true as const,
        data: {
          connection_id: connectionId,
          connection_token: issued.token,
          expires_in: options.ttlSeconds,
          signaling_url: toWebSocketUrl(options.publicUrl, '/ws/client'),
          relay_url: toWebSocketUrl(options.publicUrl, '/ws/relay')
        }
      }
    },

    iceConfig(turn: { enabled: boolean; urls: string[]; username: string; credential: string }) {
      return {
        ice_servers: turn.enabled
          ? [{ urls: turn.urls }, { urls: turn.urls, username: turn.username, credential: turn.credential }]
          : []
      }
    }
  }
}
