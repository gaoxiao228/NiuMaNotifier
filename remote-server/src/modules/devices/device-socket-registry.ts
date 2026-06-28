export type ClosableDeviceSocket = {
  close(code: number, reason: string): void
}

export function createDeviceSocketRegistry() {
  const sockets = new Map<string, ClosableDeviceSocket>()

  return {
    add(deviceId: string, socket: ClosableDeviceSocket) {
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
    }
  }
}

export type DeviceSocketRegistry = ReturnType<typeof createDeviceSocketRegistry>
