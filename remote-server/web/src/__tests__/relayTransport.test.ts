import { describe, expect, it } from 'vitest'

import {
  buildRelaySocketUrl,
  decodeRelayPayload,
  encodeRelayPayload,
  type RelayFrame
} from '../remote/relayTransport.js'

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
})
