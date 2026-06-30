export type RelaySide = 'client' | 'device'

export type RelayFrame = {
  version: 1
  type: 'relay.frame'
  id: string
  connection_id: string
  seq: number
  ciphertext: string
}

export type RelayReadyMessage = {
  version: 1
  type: 'relay.ready'
  connection_id: string
}

export type RelaySocketBind = {
  connection_id: string
  connection_token: string
  side: RelaySide
}

export type RelayClientOptions = {
  url: string
  connectionId: string
  WebSocketImpl?: typeof WebSocket
  onOpen(): void
  onReady(): void
  onPayload(value: unknown): void
  onClose(): void
  onError(error: Error): void
}

export type RelayClient = {
  socket: WebSocket
  send(value: unknown): void
  close(): void
}

function bytesToBase64(bytes: Uint8Array): string {
  if (typeof Buffer !== 'undefined') return Buffer.from(bytes).toString('base64')

  let binary = ''
  for (const byte of bytes) binary += String.fromCharCode(byte)
  return btoa(binary)
}

function base64ToBytes(payload: string): Uint8Array {
  if (typeof Buffer !== 'undefined') return new Uint8Array(Buffer.from(payload, 'base64'))

  const binary = atob(payload)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index)
  }
  return bytes
}

export function encodeRelayPayload(value: unknown): string {
  // Relay Task 6 阶段仍是明文 JSON，只是字段名沿用 ciphertext 以兼容服务端 schema。
  return bytesToBase64(new TextEncoder().encode(JSON.stringify(value)))
}

export function decodeRelayPayload(payload: string): unknown {
  const json = new TextDecoder().decode(base64ToBytes(payload))
  return JSON.parse(json) as unknown
}

export function createRelayFrame(connectionId: string, seq: number, payloadValue: unknown): RelayFrame {
  return {
    version: 1,
    type: 'relay.frame',
    id: `relay_${seq}`,
    connection_id: connectionId,
    seq,
    ciphertext: encodeRelayPayload(payloadValue)
  }
}

export function buildRelaySocketUrl(baseUrlOrWsUrl: string, bind: RelaySocketBind): string {
  const source = baseUrlOrWsUrl || window.location.origin
  const url = new URL(source, window.location.origin)

  if (url.protocol === 'http:') url.protocol = 'ws:'
  if (url.protocol === 'https:') url.protocol = 'wss:'

  if (url.protocol !== 'ws:' && url.protocol !== 'wss:') {
    throw new Error('Unsupported relay websocket URL protocol')
  }

  url.pathname = '/ws/relay'
  url.search = ''
  url.hash = ''
  url.searchParams.set('connection_id', bind.connection_id)
  url.searchParams.set('connection_token', bind.connection_token)
  url.searchParams.set('side', bind.side)
  return url.toString()
}

function parseRelayMessage(data: unknown): unknown {
  if (typeof data !== 'string') return data
  return JSON.parse(data) as unknown
}

function isRelayFrame(value: unknown): value is RelayFrame {
  if (value === null || typeof value !== 'object') return false
  const item = value as Partial<RelayFrame>
  return (
    item.version === 1 &&
    item.type === 'relay.frame' &&
    typeof item.connection_id === 'string' &&
    typeof item.seq === 'number' &&
    typeof item.ciphertext === 'string'
  )
}

function isRelayReadyMessage(value: unknown): value is RelayReadyMessage {
  if (value === null || typeof value !== 'object') return false
  const item = value as Partial<RelayReadyMessage>
  return item.version === 1 && item.type === 'relay.ready' && typeof item.connection_id === 'string'
}

function resolveOpenReadyState(WebSocketCtor: typeof WebSocket): number {
  // 注入的 WebSocket 实现可能只在构造函数或全局 WebSocket 上暴露 OPEN 常量。
  if (typeof WebSocketCtor.OPEN === 'number') return WebSocketCtor.OPEN
  if (typeof WebSocket !== 'undefined' && typeof WebSocket.OPEN === 'number') return WebSocket.OPEN
  return 1
}

export function createRelayClient(options: RelayClientOptions): RelayClient {
  const WebSocketCtor = options.WebSocketImpl ?? WebSocket
  const socket = new WebSocketCtor(options.url)
  const openReadyState = resolveOpenReadyState(WebSocketCtor)
  let active = true
  let closedByClient = false
  let nextSeq = 1

  socket.onopen = () => {
    if (active) options.onOpen()
  }
  socket.onmessage = (event) => {
    if (!active) return
    try {
      const frame = parseRelayMessage(event.data)
      if (isRelayReadyMessage(frame)) {
        if (frame.connection_id === options.connectionId) options.onReady()
        return
      }
      if (!isRelayFrame(frame) || frame.connection_id !== options.connectionId) return
      options.onPayload(decodeRelayPayload(frame.ciphertext))
    } catch (err) {
      options.onError(err instanceof Error ? err : new Error('Relay payload decode failed'))
    }
  }
  socket.onerror = () => {
    if (active) options.onError(new Error('Relay websocket error'))
  }
  socket.onclose = () => {
    if (!active) return
    active = false
    if (!closedByClient) options.onClose()
  }

  return {
    socket,
    send(value) {
      if (!active) return
      if (socket.readyState !== openReadyState) {
        throw new Error('Relay websocket is not open')
      }
      const frame = createRelayFrame(options.connectionId, nextSeq, value)
      nextSeq += 1
      socket.send(JSON.stringify(frame))
    },
    close() {
      if (!active && closedByClient) return
      closedByClient = true
      active = false
      socket.close()
    }
  }
}
