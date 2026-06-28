export type DeviceSocket = {
  close(code: number, reason: string): void
  send?(data: string): void
}

export function createDeviceSocketRegistry() {
  const sockets = new Map<string, DeviceSocket>()
  // Web client socket 按 connection_id 绑定，用于把设备响应定向回发给发起方。
  const clientSockets = new Map<string, DeviceSocket>()

  return {
    add(deviceId: string, socket: DeviceSocket) {
      sockets.set(deviceId, socket)
    },
    remove(deviceId: string) {
      sockets.delete(deviceId)
    },
    has(deviceId: string) {
      return sockets.has(deviceId)
    },
    closeDevice(deviceId: string, code: number, reason: string) {
      const socket = sockets.get(deviceId)
      if (!socket) return false

      socket.close(code, reason)
      sockets.delete(deviceId)
      return true
    },
    sendToDevice(deviceId: string, message: object) {
      const socket = sockets.get(deviceId)
      if (!socket?.send) return false

      socket.send(JSON.stringify(message))
      return true
    },
    bindClient(connectionId: string, socket: DeviceSocket) {
      clientSockets.set(connectionId, socket)
    },
    unbindClient(connectionId: string) {
      clientSockets.delete(connectionId)
    },
    sendToClient(connectionId: string, message: object) {
      const socket = clientSockets.get(connectionId)
      if (!socket?.send) return false

      socket.send(JSON.stringify(message))
      return true
    }
  }
}

export type DeviceSocketRegistry = ReturnType<typeof createDeviceSocketRegistry>
