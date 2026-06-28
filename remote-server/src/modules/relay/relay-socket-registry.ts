import type { RelaySide } from './relay.schemas.js'

export type RelaySocket = {
  send(data: string): void
  close(code: number, reason: string): void
}

export type RelaySocketEntry = {
  connectionId: string
  side: RelaySide
  socketId: string
  socket: RelaySocket
}

function sideKey(connectionId: string, side: RelaySide) {
  return `${connectionId}:${side}`
}

function oppositeSide(side: RelaySide): RelaySide {
  return side === 'client' ? 'device' : 'client'
}

export function createRelaySocketRegistry() {
  const sockets = new Map<string, RelaySocketEntry>()
  const lastSeq = new Map<string, number>()

  return {
    add(entry: RelaySocketEntry) {
      sockets.set(sideKey(entry.connectionId, entry.side), entry)
    },

    remove(connectionId: string, side: RelaySide) {
      sockets.delete(sideKey(connectionId, side))
    },

    getSocketId(connectionId: string, side: RelaySide) {
      return sockets.get(sideKey(connectionId, side))?.socketId ?? null
    },

    forward(connectionId: string, fromSide: RelaySide, message: object) {
      const target = sockets.get(sideKey(connectionId, oppositeSide(fromSide)))
      if (!target) return false

      target.socket.send(JSON.stringify(message))
      return true
    },

    acceptSeq(connectionId: string, side: RelaySide, seq: number) {
      const key = sideKey(connectionId, side)
      const previous = lastSeq.get(key) ?? 0
      if (seq <= previous) return false

      lastSeq.set(key, seq)
      return true
    },

    closeConnection(connectionId: string, code: number, reason: string) {
      for (const side of ['client', 'device'] as const) {
        const key = sideKey(connectionId, side)
        const entry = sockets.get(key)
        if (entry) {
          entry.socket.close(code, reason)
          sockets.delete(key)
        }
      }
    }
  }
}

export type RelaySocketRegistry = ReturnType<typeof createRelaySocketRegistry>
