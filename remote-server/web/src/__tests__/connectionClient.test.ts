import { describe, expect, it, vi } from 'vitest'

import { buildClientSocketUrl, createConnectionClient } from '../remote/connectionClient.js'

const bind = {
  connection_id: 'conn_123',
  connection_token: 'short_token'
}

describe('buildClientSocketUrl', () => {
  it('converts an http origin to a client websocket URL', () => {
    expect(buildClientSocketUrl('http://127.0.0.1:27880', bind)).toBe(
      'ws://127.0.0.1:27880/ws/client?connection_id=conn_123&connection_token=short_token'
    )
  })

  it('converts an https base path to a root client websocket URL', () => {
    expect(buildClientSocketUrl('https://example.com/base', bind)).toBe(
      'wss://example.com/ws/client?connection_id=conn_123&connection_token=short_token'
    )
  })

  it('appends binding query to a websocket signaling URL', () => {
    expect(buildClientSocketUrl('ws://127.0.0.1:27880/ws/client', bind)).toBe(
      'ws://127.0.0.1:27880/ws/client?connection_id=conn_123&connection_token=short_token'
    )
  })

  it('never includes an account access token in the websocket URL', () => {
    const url = buildClientSocketUrl('https://example.com?access_token=account-token', bind)

    expect(url).not.toContain('access_token')
    expect(url).not.toContain('account-token')
  })
})

describe('createConnectionClient', () => {
  it('maps accepted, rejected, invalid json, and close events without throwing', () => {
    const statuses: string[] = []
    const messages: unknown[] = []

    class FakeWebSocket {
      onmessage: ((event: MessageEvent<string>) => void) | null = null
      onclose: (() => void) | null = null
      onerror: (() => void) | null = null

      constructor(public url: string) {}

      close = vi.fn(() => {
        this.onclose?.()
      })
    }

    const client = createConnectionClient({
      url: 'ws://127.0.0.1/ws/client',
      WebSocketImpl: FakeWebSocket as unknown as typeof WebSocket,
      onStatus: (status) => statuses.push(status),
      onMessage: (value) => messages.push(value)
    })

    const socket = client.socket as unknown as FakeWebSocket
    socket.onmessage?.({ data: JSON.stringify({ type: 'connection.accept' }) } as MessageEvent<string>)
    socket.onmessage?.({ data: JSON.stringify({ type: 'connection.reject' }) } as MessageEvent<string>)
    expect(() => socket.onmessage?.({ data: '{bad-json' } as MessageEvent<string>)).not.toThrow()
    client.close()

    expect(socket.url).toBe('ws://127.0.0.1/ws/client')
    expect(statuses).toEqual(['connecting', 'accepted', 'rejected', 'closed'])
    expect(messages).toEqual([{ type: 'connection.accept' }, { type: 'connection.reject' }, '{bad-json'])
  })
})
