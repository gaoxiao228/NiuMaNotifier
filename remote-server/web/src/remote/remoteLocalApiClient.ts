import type { RemoteTransportKind } from './plainRpcClient.js'

type RpcLike = {
  request(method: string, params?: unknown): Promise<unknown>
}

type RemoteLocalApiRequest = {
  method: string
  path: string
  headers?: Record<string, string>
  body?: unknown
}

type StreamEvent = {
  event: string
  id: string | null
  data: unknown
  seq?: number
  observedTransport?: RemoteTransportKind
  declaredTransport?: RemoteTransportKind
}

type StreamHandlers = {
  onEvent(event: StreamEvent): void
  onClosed?(reason: string): void
  onError?(error: Error): void
}

type StreamHandle = {
  streamId: string
  close(): Promise<void>
}

type NotificationMetadata = {
  observedTransport?: RemoteTransportKind
  declaredTransport?: RemoteTransportKind
}

type RegisteredStream = {
  handlers: StreamHandlers
  lastSeq: number | null
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export function createRemoteLocalApiClient(rpc: RpcLike) {
  const streams = new Map<string, RegisteredStream>()

  return {
    request(input: RemoteLocalApiRequest) {
      return rpc.request('local_api.request', {
        method: input.method,
        path: input.path,
        headers: input.headers ?? {},
        body: input.body ?? null
      })
    },
    get(path: string) {
      return this.request({ method: 'GET', path, body: null })
    },
    post(path: string, body: unknown) {
      return this.request({ method: 'POST', path, body })
    },
    async stream(path: string, handlers: StreamHandlers): Promise<StreamHandle> {
      const response = await rpc.request('local_api.stream', {
        method: 'GET',
        path,
        headers: {},
        body: null
      })
      if (!isRecord(response) || typeof response.stream_id !== 'string') {
        throw new Error('Invalid stream response')
      }

      const streamId = response.stream_id
      streams.set(streamId, { handlers, lastSeq: null })
      return {
        streamId,
        async close() {
          streams.delete(streamId)
          await rpc.request('local_api.stream.close', { stream_id: streamId })
        }
      }
    },
    handleNotification(method: string, params: unknown, metadata: NotificationMetadata = {}) {
      if (!isRecord(params) || typeof params.stream_id !== 'string') return
      const stream = streams.get(params.stream_id)
      if (!stream) return

      if (method === 'local_api.stream.event') {
        const seq = typeof params.seq === 'number' ? params.seq : undefined
        if (typeof seq === 'number') {
          if (stream.lastSeq !== null && seq <= stream.lastSeq) return
          stream.lastSeq = seq
        }
        stream.handlers.onEvent({
          event: typeof params.event === 'string' ? params.event : 'message',
          id: typeof params.id === 'string' ? params.id : null,
          data: params.data,
          seq,
          observedTransport: metadata.observedTransport,
          declaredTransport: metadata.declaredTransport
        })
        return
      }

      if (method === 'local_api.stream.closed') {
        streams.delete(params.stream_id)
        stream.handlers.onClosed?.(typeof params.reason === 'string' ? params.reason : 'closed')
      }
    }
  }
}
