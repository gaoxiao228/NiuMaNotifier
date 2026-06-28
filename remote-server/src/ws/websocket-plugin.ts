import websocket from '@fastify/websocket'
import type { FastifyInstance } from 'fastify'

const registrationByApp = new WeakMap<FastifyInstance, PromiseLike<void>>()

export function ensureWebsocketRegistered(app: FastifyInstance) {
  const existing = registrationByApp.get(app)
  if (existing) return

  // Fastify 插件注册会进入 app 自身的启动队列；这里提前占位，避免多个 socket 模块并发重复注册。
  registrationByApp.set(app, Promise.resolve())
  app.register(websocket)
}
