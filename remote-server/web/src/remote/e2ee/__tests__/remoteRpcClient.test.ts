import { describe, expect, it } from 'vitest'
import { createRemoteRpcClient } from '../remoteRpcClient.js'

describe('remote rpc client', () => {
  it('resolves matching response and rejects duplicate pending id', async () => {
    const sent: unknown[] = []
    const client = createRemoteRpcClient({
      timeoutMs: 1000,
      sendEncrypted: async (payload) => {
        sent.push(payload)
      }
    })

    const pending = client.request('device.get_health', {})
    expect(() => client.registerPendingForTest('req_fixed')).not.toThrow()
    expect(() => client.registerPendingForTest('req_fixed')).toThrow('duplicate request id')

    client.resolveForTest({
      version: 1,
      type: 'response',
      id: client.lastRequestIdForTest(),
      ok: true,
      result: { status: 'ok' }
    })

    await expect(pending).resolves.toEqual({ status: 'ok' })
    expect(sent[0]).toMatchObject({ type: 'request', method: 'device.get_health' })
  })
})
