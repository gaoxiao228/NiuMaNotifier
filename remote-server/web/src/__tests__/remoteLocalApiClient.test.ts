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
      event: 'session_project_groups',
      data: { list: [] }
    })
    await stream.close()

    expect(onEvent).toHaveBeenCalledWith({
      event: 'session_project_groups',
      id: null,
      data: { list: [] }
    })
    expect(rpc.request).toHaveBeenLastCalledWith('local_api.stream.close', { stream_id: 'stream_1' })
  })
})
