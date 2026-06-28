import Fastify from 'fastify'
import { describe, expect, it } from 'vitest'
import { ensureWebsocketRegistered } from '../src/ws/websocket-plugin.js'

describe('websocket plugin registration', () => {
  it('allows websocket routes to share one fastify plugin registration', async () => {
    const app = Fastify()

    expect(ensureWebsocketRegistered(app)).toBeUndefined()
    expect(ensureWebsocketRegistered(app)).toBeUndefined()
    await expect(app.ready()).resolves.toBeDefined()

    await app.close()
  })

  it('deduplicates concurrent websocket plugin registration', async () => {
    const app = Fastify()

    expect(ensureWebsocketRegistered(app)).toBeUndefined()
    expect(ensureWebsocketRegistered(app)).toBeUndefined()
    expect(ensureWebsocketRegistered(app)).toBeUndefined()
    await expect(app.ready()).resolves.toBeDefined()

    await app.close()
  })
})
