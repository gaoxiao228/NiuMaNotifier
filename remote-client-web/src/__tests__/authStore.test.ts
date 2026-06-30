import { afterEach, describe, expect, it, vi } from 'vitest'

import { createAuthStore } from '../auth/authStore.js'

function installStorage(initial: Record<string, string> = {}) {
  const data = new Map(Object.entries(initial))
  const storage: Storage = {
    get length() {
      return data.size
    },
    clear() {
      data.clear()
    },
    getItem(key) {
      return data.get(key) ?? null
    },
    key(index) {
      return Array.from(data.keys())[index] ?? null
    },
    removeItem(key) {
      data.delete(key)
    },
    setItem(key, value) {
      data.set(key, value)
    }
  }
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: storage
  })
  return storage
}

afterEach(() => {
  vi.restoreAllMocks()
})

describe('createAuthStore', () => {
  it('persists and clears login sessions', () => {
    const storage = installStorage()
    const store = createAuthStore('test.auth')

    store.setSession({
      accessToken: 'access-token',
      refreshToken: 'refresh-token',
      user: { id: 'user-1', email: 'user@example.com', role: 'user', status: 'active' }
    })

    expect(createAuthStore('test.auth').getSnapshot()).toEqual({
      accessToken: 'access-token',
      refreshToken: 'refresh-token',
      user: { id: 'user-1', email: 'user@example.com', role: 'user', status: 'active' }
    })

    store.clear()
    expect(store.getSnapshot()).toEqual({ accessToken: null, refreshToken: null, user: null })
    expect(storage.getItem('test.auth')).toBeNull()
  })

  it('clears damaged JSON and falls back to an empty session', () => {
    const storage = installStorage({ 'test.auth': '{broken-json' })
    const store = createAuthStore('test.auth')

    expect(store.getSnapshot()).toEqual({ accessToken: null, refreshToken: null, user: null })
    expect(storage.getItem('test.auth')).toBeNull()
  })

  it('notifies subscribers when the session changes', () => {
    installStorage()
    const store = createAuthStore('test.auth')
    const listener = vi.fn()
    const unsubscribe = store.subscribe(listener)

    store.setSession({
      accessToken: 'access-token',
      refreshToken: 'refresh-token',
      user: { id: 'user-1', email: 'user@example.com', role: 'user', status: 'active' }
    })
    store.clear()
    unsubscribe()
    store.setSession({
      accessToken: 'next-token',
      refreshToken: 'next-refresh-token',
      user: { id: 'user-2', email: 'next@example.com', role: 'user', status: 'active' }
    })

    expect(listener).toHaveBeenCalledTimes(2)
    expect(listener).toHaveBeenNthCalledWith(1, {
      accessToken: 'access-token',
      refreshToken: 'refresh-token',
      user: { id: 'user-1', email: 'user@example.com', role: 'user', status: 'active' }
    })
    expect(listener).toHaveBeenNthCalledWith(2, { accessToken: null, refreshToken: null, user: null })
  })
})
