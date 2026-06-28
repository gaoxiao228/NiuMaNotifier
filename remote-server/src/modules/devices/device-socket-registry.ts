export type DeviceSocket = {
  close(code: number, reason: string): void
  send?(data: string): void
}

export function createDeviceSocketRegistry() {
  const sockets = new Map<string, DeviceSocket>()

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
    }
  }
}

export type DeviceSocketRegistry = ReturnType<typeof createDeviceSocketRegistry>
