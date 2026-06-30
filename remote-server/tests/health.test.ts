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

  it('allows configured remote client origin for browser api calls', async () => {
    const app = buildApp({ corsOrigins: ['http://127.0.0.1:27883'] })
    const response = await app.inject({
      method: 'GET',
      url: '/api/v1/health',
      headers: { origin: 'http://127.0.0.1:27883' }
    })

    expect(response.headers['access-control-allow-origin']).toBe('http://127.0.0.1:27883')
    expect(response.headers.vary).toBe('Origin')
  })

  it('allows localhost remote client origin by default', async () => {
    const app = buildApp()
    const response = await app.inject({
      method: 'OPTIONS',
      url: '/api/v1/auth/login',
      headers: {
        origin: 'http://localhost:27883',
        'access-control-request-method': 'POST'
      }
    })

    expect(response.statusCode).toBe(204)
    expect(response.headers['access-control-allow-origin']).toBe('http://localhost:27883')
  })

  it('returns empty preflight response for configured remote client origin', async () => {
    const app = buildApp({ corsOrigins: ['http://127.0.0.1:27883'] })
    const response = await app.inject({
      method: 'OPTIONS',
      url: '/api/v1/auth/login',
      headers: {
        origin: 'http://127.0.0.1:27883',
        'access-control-request-method': 'POST',
        'access-control-request-headers': 'authorization,content-type'
      }
    })

    expect(response.statusCode).toBe(204)
    expect(response.body).toBe('')
    expect(response.headers['access-control-allow-origin']).toBe('http://127.0.0.1:27883')
    expect(response.headers['access-control-allow-methods']).toBe('GET,POST,OPTIONS')
    expect(response.headers['access-control-allow-headers']).toBe('Authorization,Content-Type,Accept')
  })
})
