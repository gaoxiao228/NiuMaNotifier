export type ConnectionSocketBind = {
  connection_id: string
  connection_token: string
}

export type ConnectionStatus = 'connecting' | 'accepted' | 'rejected' | 'closed' | 'error'

export type ConnectionClientOptions = {
  url: string
  WebSocketImpl?: typeof WebSocket
  onStatus(status: ConnectionStatus): void
  onMessage(value: unknown): void
}

export type ConnectionClient = {
  socket: WebSocket
  close(): void
}

function normalizeSocketBaseUrl(baseUrlOrWsUrl: string): URL {
  const source = baseUrlOrWsUrl || window.location.origin
  const url = new URL(source, window.location.origin)

  if (url.protocol === 'http:') url.protocol = 'ws:'
  if (url.protocol === 'https:') url.protocol = 'wss:'

  if (url.protocol !== 'ws:' && url.protocol !== 'wss:') {
    throw new Error('Unsupported websocket URL protocol')
  }

  // HTTP(S) 输入可能包含页面路径，客户端信令入口固定在根路径 /ws/client。
  if (source.startsWith('http://') || source.startsWith('https://') || source.startsWith('/')) {
    url.pathname = '/ws/client'
  } else if (!url.pathname || url.pathname === '/') {
    url.pathname = '/ws/client'
  }

  url.search = ''
  url.hash = ''
  return url
}

export function buildClientSocketUrl(baseUrlOrWsUrl: string, bind: ConnectionSocketBind): string {
  const url = normalizeSocketBaseUrl(baseUrlOrWsUrl)
  url.searchParams.set('connection_id', bind.connection_id)
  url.searchParams.set('connection_token', bind.connection_token)
  return url.toString()
}

function parseSignalMessage(data: unknown): unknown {
  if (typeof data !== 'string') return data
  try {
    return JSON.parse(data) as unknown
  } catch {
    return data
  }
}

function signalType(value: unknown): string | null {
  if (!value || typeof value !== 'object' || !('type' in value)) return null
  const type = (value as { type?: unknown }).type
  return typeof type === 'string' ? type : null
}

export function createConnectionClient(options: ConnectionClientOptions): ConnectionClient {
  const WebSocketCtor = options.WebSocketImpl ?? WebSocket
  const socket = new WebSocketCtor(options.url)

  options.onStatus('connecting')

  socket.onmessage = (event) => {
    const value = parseSignalMessage(event.data)
    options.onMessage(value)

    const type = signalType(value)
    if (type === 'connection.accept') options.onStatus('accepted')
    if (type === 'connection.reject') options.onStatus('rejected')
  }
  socket.onerror = () => options.onStatus('error')
  socket.onclose = () => options.onStatus('closed')

  return {
    socket,
    close() {
      socket.close()
    }
  }
}
