import { cleanup, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { App } from '../App.js'
import { messages } from '../i18n/messages.js'

function createStorage(initialToken: string): Storage {
  const data = new Map<string, string>([['niuma.remote.access_token', initialToken]])
  return {
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
}

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('App auth state', () => {
  it.each([
    { code: 200001, message: '未登录' },
    { code: 200002, message: 'Token 无效' },
    { code: 200003, message: 'Token 已过期' }
  ])('clears persisted token and returns to login when API returns auth code $code', async ({ code, message }) => {
    const storage = createStorage('expired-token')
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: storage
    })
    vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(JSON.stringify({ code, message, data: null }), {
        headers: { 'Content-Type': 'application/json' }
      })
    )

    render(<App />)

    await waitFor(() => expect(storage.getItem('niuma.remote.access_token')).toBeNull())
    expect(screen.getByRole('textbox', { name: messages.en.email })).not.toBeNull()
  })
})
