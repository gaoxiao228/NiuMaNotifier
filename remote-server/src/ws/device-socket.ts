import type { FastifyInstance } from 'fastify'
import { loadConfigFromEnv } from '../config.js'
import { createDb } from '../db/client.js'
import {
  createDeviceSocketRegistry,
  type DeviceSocketRegistry
} from '../modules/devices/device-socket-registry.js'
import {
  createDeviceTokenService,
  type AuthenticatedDevice
} from '../modules/devices/device-token.service.js'
import { createDevicesRepository } from '../modules/devices/devices.repository.js'
import { createPresenceService } from '../modules/devices/presence.service.js'
import { createRedis } from '../redis/client.js'
import { createPublicId } from '../shared/id.js'
import { ensureWebsocketRegistered } from './websocket-plugin.js'
import { deviceSocketMessageSchema } from './ws-message.schemas.js'

export type DeviceMessageDeps = {
  raw: string
  authenticatedDevice: Pick<AuthenticatedDevice, 'id' | 'userId'>
  socketId: string
  serverInstanceId: string
  presence: {
    markOnline(input: {
      userId: string
      deviceId: string
      socketId: string
      serverInstanceId: string
      lastSeenAt: string
      capabilities: unknown
    }): Promise<void>
  }
  devices: {
    updateLastSeen(deviceId: string, lastSeenAt: Date, capabilities: unknown): Promise<void>
  }
}

export async function handleDeviceMessage(deps: DeviceMessageDeps) {
  const message = deviceSocketMessageSchema.parse(JSON.parse(deps.raw))
  const now = new Date()

  if (message.type === 'device.hello') {
    if (message.data.device_id !== deps.authenticatedDevice.id) {
      throw new Error('device_id_mismatch')
    }

    await deps.presence.markOnline({
      userId: deps.authenticatedDevice.userId,
      deviceId: deps.authenticatedDevice.id,
      socketId: deps.socketId,
      serverInstanceId: deps.serverInstanceId,
      lastSeenAt: now.toISOString(),
      capabilities: message.data.capabilities
    })
    await deps.devices.updateLastSeen(deps.authenticatedDevice.id, now, message.data.capabilities)
    return
  }

  await deps.presence.markOnline({
    userId: deps.authenticatedDevice.userId,
    deviceId: deps.authenticatedDevice.id,
    socketId: deps.socketId,
    serverInstanceId: deps.serverInstanceId,
    lastSeenAt: now.toISOString(),
      capabilities: {}
    })
  await deps.devices.updateLastSeen(deps.authenticatedDevice.id, now, {})
}

export async function registerDeviceSocket(
  app: FastifyInstance,
  registry: DeviceSocketRegistry = createDeviceSocketRegistry()
) {
  await ensureWebsocketRegistered(app)

  const config = loadConfigFromEnv()
  const { db } = createDb(config.databaseUrl)
  const redis = createRedis(config.redisUrl)
  const devicesRepo = createDevicesRepository(db)
  const deviceTokenService = createDeviceTokenService({
    repo: devicesRepo,
    tokenPepper: config.tokenPepper
  })
  const presence = createPresenceService({
    redis,
    ttlSeconds: config.devicePresenceTtlSeconds
  })
  const serverInstanceId = `srv_${process.pid}`

  app.get('/ws/device', { websocket: true }, async (socket, request) => {
    const auth = await deviceTokenService.authenticate(request.headers.authorization)
    if (!auth.ok) {
      socket.close(4001, JSON.stringify({ code: auth.code, message: auth.message }))
      return
    }

    const socketId = createPublicId('sock')
    registry.add(auth.device.id, socket)

    socket.on('message', async (raw: { toString(): string }) => {
      try {
        await handleDeviceMessage({
          raw: raw.toString(),
          authenticatedDevice: auth.device,
          socketId,
          serverInstanceId,
          presence,
          devices: devicesRepo
        })
      } catch {
        socket.close(4002, 'invalid_device_message')
      }
    })

    socket.on('close', async () => {
      registry.remove(auth.device.id)
      await presence.markOffline(auth.device.id)
    })
  })
}
