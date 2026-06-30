import { describe, expect, it } from 'vitest'

import { createAuthApi, type MeResponse } from '../api/authApi.js'
import type { HttpClient } from '../api/httpClient.js'

describe('createAuthApi', () => {
  it('requests current user from auth me endpoint and keeps the documented response shape', async () => {
    const response: MeResponse = {
      user: {
        id: 'user-1',
        email: 'user@example.com',
        role: 'user',
        status: 'active'
      }
    }
    const requestedPaths: string[] = []
    const http: HttpClient = {
      async get<T>(path: string) {
        requestedPaths.push(path)
        return response as T
      },
      async post<T>() {
        return undefined as T
      }
    }
    const api = createAuthApi(http)
    const result: Promise<MeResponse> = api.me()

    await expect(result).resolves.toEqual(response)
    expect(requestedPaths).toEqual(['/api/v1/auth/me'])
  })
})
