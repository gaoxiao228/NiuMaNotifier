export type RelaySide = 'client' | 'device'

export type RelayFrame = {
  version: 1
  type: 'relay.frame'
  id: string
  connection_id: string
  seq: number
  ciphertext: string
}

export type RelaySocketBind = {
  connection_id: string
  connection_token: string
  side: RelaySide
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
