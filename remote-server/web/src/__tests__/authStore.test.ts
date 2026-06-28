import { describe, expect, it } from 'vitest'

import { createMemoryAuthStore } from '../auth/authStore.js'

describe('createMemoryAuthStore', () => {
  it('sets, gets, and clears the auth token', () => {
    const store = createMemoryAuthStore()

    expect(store.getToken()).toBeNull()

    store.setToken('access-token')
    expect(store.getToken()).toBe('access-token')

    store.clearToken()
    expect(store.getToken()).toBeNull()
  })
})
