import { describe, expect, it, vi } from 'vitest'

import { createRemoteLocalApiClient } from '../remote/remoteLocalApiClient.js'

describe('createRemoteLocalApiClient', () => {
  it('sends local_api.request for get calls', async () => {
    const rpc = {
      request: vi.fn().mockResolvedValue({
        status: 200,
        headers: {},
        body: { code: 0, message: 'ok', data: {} }
      })
    }
    const client = createRemoteLocalApiClient(rpc)

    await client.get('/api/v1/session_project_groups?tool=codex')

    expect(rpc.request).toHaveBeenCalledWith('local_api.request', {
      method: 'GET',
      path: '/api/v1/session_project_groups?tool=codex',
      headers: {},
      body: null
    })
  })

  it('registers stream handlers and closes stream by id', async () => {
    const rpc = {
      request: vi
        .fn()
        .mockResolvedValueOnce({ stream_id: 'stream_1' })
        .mockResolvedValueOnce({ closed: true })
    }
    const client = createRemoteLocalApiClient(rpc)
    const onEvent = vi.fn()

    const stream = await client.stream('/api/v1/session_project_groups/stream?tool=codex', { onEvent })
    client.handleNotification('local_api.stream.event', {
      stream_id: 'stream_1',
      seq: 1,
      event: 'session_project_groups',
      data: { list: [] }
    }, { observedTransport: 'relay', declaredTransport: 'relay' })
    await stream.close()

    expect(onEvent).toHaveBeenCalledWith({
      event: 'session_project_groups',
      id: null,
      data: { list: [] },
      seq: 1,
      observedTransport: 'relay',
      declaredTransport: 'relay'
    })
    expect(rpc.request).toHaveBeenLastCalledWith('local_api.stream.close', { stream_id: 'stream_1' })
  })

  it('ignores stale stream events by sequence number', async () => {
    const rpc = {
      request: vi.fn().mockResolvedValue({ stream_id: 'stream_1' })
    }
    const client = createRemoteLocalApiClient(rpc)
    const onEvent = vi.fn()

    await client.stream('/api/v1/session_project_groups/stream?tool=codex', { onEvent })
    client.handleNotification('local_api.stream.event', {
      stream_id: 'stream_1',
      seq: 2,
      event: 'session_project_groups',
      data: { list: ['new'] }
    }, { observedTransport: 'webrtc', declaredTransport: 'webrtc' })
    client.handleNotification('local_api.stream.event', {
      stream_id: 'stream_1',
      seq: 1,
      event: 'session_project_groups',
      data: { list: ['old'] }
    }, { observedTransport: 'relay', declaredTransport: 'relay' })

    expect(onEvent).toHaveBeenCalledTimes(1)
    expect(onEvent).toHaveBeenCalledWith({
      event: 'session_project_groups',
      id: null,
      data: { list: ['new'] },
      seq: 2,
      observedTransport: 'webrtc',
      declaredTransport: 'webrtc'
    })
  })
})
