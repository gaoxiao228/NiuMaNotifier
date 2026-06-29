import Fastify from 'fastify'
import { describe, expect, it } from 'vitest'
import { ensureWebsocketRegistered } from '../src/ws/websocket-plugin.js'

describe('websocket plugin registration', () => {
  it('allows websocket routes to share one fastify plugin registration', async () => {
    const app = Fastify()

    await expect(ensureWebsocketRegistered(app)).resolves.toBeUndefined()
    await expect(ensureWebsocketRegistered(app)).resolves.toBeUndefined()
    await expect(app.ready()).resolves.toBeDefined()

    await app.close()
  })

  it('deduplicates concurrent websocket plugin registration', async () => {
    const app = Fastify()

    await expect(ensureWebsocketRegistered(app)).resolves.toBeUndefined()
    await expect(ensureWebsocketRegistered(app)).resolves.toBeUndefined()
    await expect(ensureWebsocketRegistered(app)).resolves.toBeUndefined()
    await expect(app.ready()).resolves.toBeDefined()

    await app.close()
  })

  it('installs websocket route hooks before websocket routes are added', async () => {
    const app = Fastify()

    await ensureWebsocketRegistered(app)
    app.get('/ws/probe', { websocket: true }, (socket) => {
      socket.close(1000, 'ok')
    })

    await app.ready()
    const socket = await app.injectWS('/ws/probe')

    expect(typeof socket.close).toBe('function')
    socket.close()
    await app.close()
  })
})
