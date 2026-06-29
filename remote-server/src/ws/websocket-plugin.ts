import websocket from '@fastify/websocket'
import type { FastifyInstance } from 'fastify'

const registrationByApp = new WeakMap<FastifyInstance, Promise<void>>()

export function ensureWebsocketRegistered(app: FastifyInstance): Promise<void> {
  const existing = registrationByApp.get(app)
  if (existing) return existing

  // Fastify 的 onRoute hook 只有在插件注册完成后才会安装，socket 路由必须等待这个屏障。
  const registration = Promise.resolve(app.register(websocket, {
    errorHandler(error, socket) {
      console.error(`NiuMaNotifier websocket error: ${error.message}`)
      // 不同升级阶段拿到的连接对象能力可能不同；错误处理不能再抛异常，否则会遮住原始问题。
      if (typeof socket.close === 'function') {
        socket.close(1011, 'websocket_error')
      } else if (typeof socket.terminate === 'function') {
        socket.terminate()
      }
    }
  })).then(() => undefined)
  registrationByApp.set(app, registration)
  return registration
}
