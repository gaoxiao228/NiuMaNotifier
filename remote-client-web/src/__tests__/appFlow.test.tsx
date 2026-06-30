import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { App } from '../App.js'

const user = {
  id: 'user-1',
  email: 'user@example.com',
  role: 'user',
  status: 'active'
}

function envelope(data: unknown, code = 0, message = 'ok') {
  return JSON.stringify({ code, message, data })
}

beforeEach(() => {
  window.localStorage?.clear()
})

afterEach(() => {
  vi.restoreAllMocks()
  window.localStorage?.clear()
})

describe('App login and device flow', () => {
  it('loads devices after a successful login', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockImplementation(async (input) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/login')) {
        return new Response(
          envelope({
            access_token: 'access-token',
            refresh_token: 'refresh-token',
            expires_at: '2026-07-01T00:00:00.000Z',
            user
          })
        )
      }
      if (url.endsWith('/api/v1/devices/list')) {
        return new Response(
          envelope({
            list: [
              {
                id: 'device-1',
                name: 'Office Mac',
                online: true,
                last_seen_at: '2026-06-30T08:00:00.000Z',
                capabilities: { screen: true },
                identity_public_key: null
              }
            ]
          })
        )
      }
      return new Response(envelope(null, 404, 'not found'), { status: 404 })
    })

    render(<App />)

    fireEvent.change(screen.getByLabelText('Email'), {
      target: { value: 'user@example.com' }
    })
    fireEvent.change(screen.getByLabelText('Password'), {
      target: { value: 'secret-password' }
    })
    fireEvent.click(screen.getByRole('button', { name: /sign in/i }))

    expect(await screen.findByText('Office Mac')).toBeInTheDocument()
    expect(fetchMock).toHaveBeenCalledWith(
      expect.stringContaining('/api/v1/devices/list'),
      expect.objectContaining({
        method: 'GET'
      })
    )
  })

  it('returns to login when the device request reports an expired token', async () => {
    vi.spyOn(globalThis, 'fetch').mockImplementation(async (input) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/login')) {
        return new Response(
          envelope({
            access_token: 'expired-access-token',
            refresh_token: 'refresh-token',
            expires_at: '2026-07-01T00:00:00.000Z',
            user
          })
        )
      }
      if (url.endsWith('/api/v1/devices/list')) {
        return new Response(envelope(null, 200003, 'Token expired'))
      }
      return new Response(envelope(null, 404, 'not found'), { status: 404 })
    })

    render(<App />)

    fireEvent.change(screen.getByLabelText('Email'), {
      target: { value: 'user@example.com' }
    })
    fireEvent.change(screen.getByLabelText('Password'), {
      target: { value: 'secret-password' }
    })
    fireEvent.click(screen.getByRole('button', { name: /sign in/i }))

    await waitFor(() => {
      expect(screen.getByRole('heading', { name: 'Sign in to remote client' })).toBeInTheDocument()
    })
  })

  it('shows a localized login error for network failures', async () => {
    vi.spyOn(globalThis, 'fetch').mockRejectedValue(new TypeError('Failed to fetch'))

    render(<App />)

    fireEvent.change(screen.getByLabelText('Email'), {
      target: { value: 'user@example.com' }
    })
    fireEvent.change(screen.getByLabelText('Password'), {
      target: { value: 'secret-password' }
    })
    fireEvent.click(screen.getByRole('button', { name: /sign in/i }))

    expect(await screen.findByText('Network connection failed. Check your connection and try again.')).toBeInTheDocument()
    expect(screen.queryByText('api_error_network')).not.toBeInTheDocument()
  })

  it('shows a localized device loading error for network failures', async () => {
    vi.spyOn(globalThis, 'fetch').mockImplementation(async (input) => {
      const url = String(input)
      if (url.endsWith('/api/v1/auth/login')) {
        return new Response(
          envelope({
            access_token: 'access-token',
            refresh_token: 'refresh-token',
            expires_at: '2026-07-01T00:00:00.000Z',
            user
          })
        )
      }
      if (url.endsWith('/api/v1/devices/list')) {
        throw new TypeError('Failed to fetch')
      }
      return new Response(envelope(null, 404, 'not found'), { status: 404 })
    })

    render(<App />)

    fireEvent.change(screen.getByLabelText('Email'), {
      target: { value: 'user@example.com' }
    })
    fireEvent.change(screen.getByLabelText('Password'), {
      target: { value: 'secret-password' }
    })
    fireEvent.click(screen.getByRole('button', { name: /sign in/i }))

    expect(await screen.findByText('Network connection failed. Check your connection and try again.')).toBeInTheDocument()
    expect(screen.getByRole('heading', { name: 'Select a device' })).toBeInTheDocument()
    expect(screen.queryByText('api_error_network')).not.toBeInTheDocument()
  })
})
