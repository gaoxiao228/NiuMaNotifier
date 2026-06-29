import type { ObservedPlainRpcMessage, RemoteTransportKind } from './plainRpcClient.js'

export type RemoteTransport = {
  kind: RemoteTransportKind
  send(value: unknown): void
  close(): void
}

type RemoteMessageBusOptions = {
  onInbound(message: ObservedPlainRpcMessage): void
}

export type RemoteMessageBus = {
  register(transport: RemoteTransport): void
  unregister(kind: RemoteTransportKind): void
  setOpen(kind: RemoteTransportKind, open: boolean): void
  send(value: unknown): void
  receive(payload: unknown, observedTransport: RemoteTransportKind): void
  close(): void
}

type TransportSlot = {
  transport: RemoteTransport
  open: boolean
}

const PREFERRED_TRANSPORTS: RemoteTransportKind[] = ['webrtc', 'relay']

export function createRemoteMessageBus(options: RemoteMessageBusOptions): RemoteMessageBus {
  const transports = new Map<RemoteTransportKind, TransportSlot>()

  function findOpenTransport(): RemoteTransport | null {
    for (const kind of PREFERRED_TRANSPORTS) {
      const slot = transports.get(kind)
      if (slot?.open) return slot.transport
    }
    return null
  }

  return {
    register(transport) {
      transports.set(transport.kind, { transport, open: false })
    },
    unregister(kind) {
      transports.delete(kind)
    },
    setOpen(kind, open) {
      const slot = transports.get(kind)
      if (slot) slot.open = open
    },
    send(value) {
      const transport = findOpenTransport()
      if (!transport) throw new Error('No remote transport is open')
      transport.send(value)
    },
    receive(payload, observedTransport) {
      options.onInbound({ payload, observedTransport })
    },
    close() {
      // MessageBus 只负责路由，不拥有 transport 生命周期；具体连接由调用方关闭。
      transports.clear()
    }
  }
}
