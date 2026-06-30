import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import { App } from '../App.js'
import { messages } from '../i18n/messages.js'

function createStorage(initialToken?: string): Storage {
  const data = new Map<string, string>(initialToken ? [['niuma.remote.access_token', initialToken]] : [])
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

  it('opens the admin device management page after login without exposing the remote console', async () => {
    const storage = createStorage()
    Object.defineProperty(window, 'localStorage', {
      configurable: true,
      value: storage
    })
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockImplementation(async (input) => {
      const url = String(input)
      if (url === '/api/v1/auth/login') {
        return new Response(
          JSON.stringify({
            code: 0,
            message: 'ok',
            data: {
              access_token: 'admin-token',
              refresh_token: 'refresh-token',
              expires_at: '2026-06-30T00:00:00Z',
              user: {
                id: 'admin-1',
                email: 'admin@example.com',
                role: 'admin',
                status: 'active'
              }
            }
          }),
          { headers: { 'Content-Type': 'application/json' } }
        )
      }
      if (url === '/api/v1/devices/list') {
        return new Response(
          JSON.stringify({
            code: 0,
            message: 'ok',
            data: {
              list: [
                {
                  id: 'device-1',
                  name: 'Desk Mac',
                  online: true,
                  last_seen_at: null,
                  capabilities: {},
                  identity_public_key: {}
                }
              ]
            }
          }),
          { headers: { 'Content-Type': 'application/json' } }
        )
      }
      throw new Error(`Unexpected request: ${url}`)
    })

    render(<App />)

    fireEvent.change(screen.getByRole('textbox', { name: messages.en.email }), {
      target: { value: 'admin@example.com' }
    })
    fireEvent.change(screen.getByLabelText(messages.en.password), {
      target: { value: 'password' }
    })
    fireEvent.click(screen.getByRole('button', { name: messages.en.login }))

    expect(await screen.findByText('Desk Mac')).not.toBeNull()
    expect(screen.queryByText(messages.en.device_console)).toBeNull()
    expect(screen.queryByText(messages.en.remote_sessions)).toBeNull()
    expect(screen.queryByRole('button', { name: 'Connect Desk Mac' })).toBeNull()
    expect(fetchMock).not.toHaveBeenCalledWith(expect.stringContaining('/api/v1/connections'), expect.anything())
  })
})
