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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export function createRemoteLocalApiClient(rpc: RpcLike) {
  const streams = new Map<string, StreamHandlers>()

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
      streams.set(streamId, handlers)
      return {
        streamId,
        async close() {
          streams.delete(streamId)
          await rpc.request('local_api.stream.close', { stream_id: streamId })
        }
      }
    },
    handleNotification(method: string, params: unknown) {
      if (!isRecord(params) || typeof params.stream_id !== 'string') return
      const handlers = streams.get(params.stream_id)
      if (!handlers) return

      if (method === 'local_api.stream.event') {
        handlers.onEvent({
          event: typeof params.event === 'string' ? params.event : 'message',
          id: typeof params.id === 'string' ? params.id : null,
          data: params.data
        })
        return
      }

      if (method === 'local_api.stream.closed') {
        streams.delete(params.stream_id)
        handlers.onClosed?.(typeof params.reason === 'string' ? params.reason : 'closed')
      }
    }
  }
}
