import { describe, expect, it, vi } from 'vitest'

import {
  buildRelaySocketUrl,
  createRelayClient,
  createRelayFrame,
  decodeRelayPayload,
  encodeRelayPayload,
  type RelayFrame
} from '../remote/relayTransport.js'

class FakeWebSocket {
  static OPEN = 1
  static instances: FakeWebSocket[] = []

  onopen: (() => void) | null = null
  onmessage: ((event: { data: unknown }) => void) | null = null
  onerror: (() => void) | null = null
  onclose: (() => void) | null = null
  readyState = 0
  sent: string[] = []
  closed = false

  constructor(readonly url: string) {
    FakeWebSocket.instances.push(this)
  }

  send(data: string) {
    this.sent.push(data)
  }

  close() {
    this.closed = true
  }
}

describe('relayTransport payload codec', () => {
  it('round-trips ping payloads with UTF-8 content', () => {
    const encoded = encodeRelayPayload({ type: 'ping', label: '中文 ping' })

    expect(encoded).toMatch(/^[A-Za-z0-9+/]+={0,2}$/)
    expect(decodeRelayPayload(encoded)).toEqual({ type: 'ping', label: '中文 ping' })
  })
})

describe('buildRelaySocketUrl', () => {
  const bind = {
    connection_id: 'conn_123',
    connection_token: 'cnt_valid_token_with_enough_length_123456',
    side: 'client' as const
  }

  it('converts an https base URL to the relay websocket endpoint', () => {
    expect(buildRelaySocketUrl('https://example.com/app?old=1#hash', bind)).toBe(
      'wss://example.com/ws/relay?connection_id=conn_123&connection_token=cnt_valid_token_with_enough_length_123456&side=client'
    )
  })

  it('supports websocket input and keeps only relay bind query', () => {
    expect(buildRelaySocketUrl('ws://127.0.0.1:27880/old?access_token=secret#hash', bind)).toBe(
      'ws://127.0.0.1:27880/ws/relay?connection_id=conn_123&connection_token=cnt_valid_token_with_enough_length_123456&side=client'
    )
  })

  it('never includes an account access token in the relay URL', () => {
    const url = buildRelaySocketUrl('https://example.com?access_token=account-token', bind)

    expect(url).not.toContain('access_token')
    expect(url).not.toContain('account-token')
  })
})

describe('RelayFrame', () => {
  it('matches the server relay frame schema fields', () => {
    const frame: RelayFrame = {
      version: 1,
      type: 'relay.frame',
      id: 'msg_1',
      connection_id: 'conn_123',
      seq: 1,
      ciphertext: encodeRelayPayload({ type: 'ping' })
    }

    expect(frame).toMatchObject({
      version: 1,
      type: 'relay.frame',
      connection_id: 'conn_123',
      seq: 1
    })
  })

  it('creates relay frames with encoded plain payloads', () => {
    const frame = createRelayFrame('conn_123', 2, { type: 'request', id: 'rpc_1' })

    expect(frame).toMatchObject({
      version: 1,
      type: 'relay.frame',
      id: 'relay_2',
      connection_id: 'conn_123',
      seq: 2
    })
    expect(decodeRelayPayload(frame.ciphertext)).toEqual({ type: 'request', id: 'rpc_1' })
  })
})

describe('createRelayClient', () => {
  it('sends encoded payload frames and emits decoded incoming payloads', () => {
    FakeWebSocket.instances = []
    const onOpen = vi.fn()
    const onPayload = vi.fn()
    const onClose = vi.fn()
    const onError = vi.fn()
    const onReady = vi.fn()
    const client = createRelayClient({
      url: 'ws://relay.example.com/ws/relay',
      connectionId: 'conn_123',
      WebSocketImpl: FakeWebSocket as unknown as typeof WebSocket,
      onOpen,
      onReady,
      onPayload,
      onClose,
      onError
    })
    const socket = FakeWebSocket.instances[0]

    socket.readyState = FakeWebSocket.OPEN
    socket.onopen?.()
    socket.onmessage?.({
      data: JSON.stringify({ version: 1, type: 'relay.ready', connection_id: 'conn_123' })
    })
    client.send({ version: 1, type: 'request', id: 'rpc_1', method: 'rpc.ping' })

    expect(onOpen).toHaveBeenCalledTimes(1)
    expect(onReady).toHaveBeenCalledTimes(1)
    expect(JSON.parse(socket.sent[0]) as RelayFrame).toMatchObject({
      version: 1,
      type: 'relay.frame',
      id: 'relay_1',
      connection_id: 'conn_123',
      seq: 1
    })

    socket.onmessage?.({
      data: JSON.stringify(createRelayFrame('conn_123', 7, { version: 1, type: 'response', id: 'rpc_1', ok: true }))
    })

    expect(onPayload).toHaveBeenCalledWith({ version: 1, type: 'response', id: 'rpc_1', ok: true })
    client.close()
    socket.onclose?.()
    expect(socket.closed).toBe(true)
    expect(onClose).not.toHaveBeenCalled()
  })

  it('rejects send before the socket is open without consuming a sequence number', () => {
    FakeWebSocket.instances = []
    const client = createRelayClient({
      url: 'ws://relay.example.com/ws/relay',
      connectionId: 'conn_123',
      WebSocketImpl: FakeWebSocket as unknown as typeof WebSocket,
      onOpen: vi.fn(),
      onReady: vi.fn(),
      onPayload: vi.fn(),
      onClose: vi.fn(),
      onError: vi.fn()
    })
    const socket = FakeWebSocket.instances[0]

    expect(() => client.send({ type: 'request', id: 'rpc_before_open' })).toThrow(
      'Relay websocket is not open'
    )
    expect(socket.sent).toHaveLength(0)

    socket.readyState = FakeWebSocket.OPEN
    socket.onopen?.()
    client.send({ type: 'request', id: 'rpc_after_open' })

    expect(JSON.parse(socket.sent[0]) as RelayFrame).toMatchObject({
      id: 'relay_1',
      seq: 1
    })
  })

  it('emits ready only for the current relay connection', () => {
    FakeWebSocket.instances = []
    const onReady = vi.fn()
    createRelayClient({
      url: 'ws://relay.example.com/ws/relay',
      connectionId: 'conn_123',
      WebSocketImpl: FakeWebSocket as unknown as typeof WebSocket,
      onOpen: vi.fn(),
      onReady,
      onPayload: vi.fn(),
      onClose: vi.fn(),
      onError: vi.fn()
    })
    const socket = FakeWebSocket.instances[0]

    socket.onmessage?.({
      data: JSON.stringify({ version: 1, type: 'relay.ready', connection_id: 'conn_other' })
    })
    expect(onReady).not.toHaveBeenCalled()

    socket.onmessage?.({
      data: JSON.stringify({ version: 1, type: 'relay.ready', connection_id: 'conn_123' })
    })
    expect(onReady).toHaveBeenCalledTimes(1)
  })
})
