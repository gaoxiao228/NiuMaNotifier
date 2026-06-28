import { describe, expect, it } from 'vitest'
import { buildApp } from '../src/app.js'

describe('health route', () => {
  it('returns standard envelope', async () => {
    const app = buildApp()
    const response = await app.inject({ method: 'GET', url: '/api/v1/health' })

    expect(response.statusCode).toBe(200)
    expect(response.json()).toEqual({
      code: 0,
      message: 'ok',
      data: {
        service: 'niuma-remote-server',
        status: 'ok'
      }
    })
  })

  it('returns standard envelope for missing route', async () => {
    const app = buildApp()
    const response = await app.inject({ method: 'GET', url: '/missing' })

    expect(response.statusCode).toBe(404)
    expect(response.json()).toEqual({
      code: 900005,
      message: '接口不存在',
      data: null
    })
  })
})
