import { describe, expect, it } from 'vitest'

import { createLocalStorageAuthStore, createMemoryAuthStore } from '../auth/authStore.js'

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

describe('createLocalStorageAuthStore', () => {
  it('falls back to memory when storage throws', () => {
    const storage = {
      getItem() {
        throw new Error('security error')
      },
      setItem() {
        throw new Error('quota error')
      },
      removeItem() {
        throw new Error('security error')
      },
      clear() {},
      key() {
        return null
      },
      length: 0
    } satisfies Storage
    const store = createLocalStorageAuthStore(storage)

    expect(store.getToken()).toBeNull()
    expect(() => store.setToken('access-token')).not.toThrow()
    expect(store.getToken()).toBe('access-token')
    expect(() => store.clearToken()).not.toThrow()
    expect(store.getToken()).toBeNull()
  })
})
