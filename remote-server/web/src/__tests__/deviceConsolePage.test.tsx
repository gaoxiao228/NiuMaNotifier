import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { RemoteDevice } from '../api/devicesApi.js'
import type { ConnectionCreateResult } from '../api/connectionsApi.js'
import { DeviceConsolePage } from '../remote/deviceConsolePage.js'
import { createTranslator } from '../i18n/index.js'

const t = createTranslator('en')

function createDevice(online: boolean): RemoteDevice {
  return {
    id: 'device-1',
    name: 'Desk Mac',
    online,
    last_seen_at: null,
    capabilities: {},
    identity_public_key: {}
  }
}

function createConnectionResult(): ConnectionCreateResult {
  return {
    connection_id: 'conn_123',
    connection_token: 'short_token',
    expires_at: '2026-06-29T00:00:00Z',
    expires_in: 60,
    signaling_url: 'ws://127.0.0.1:27880/ws/client',
    relay_url: 'https://relay.example.com'
  }
}

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('DeviceConsolePage', () => {
  it('creates a relay-first connection and opens a websocket client for an online device', async () => {
    const create = vi.fn().mockResolvedValue(createConnectionResult())
    const createConnection = vi.fn()

    render(
      <DeviceConsolePage
        device={createDevice(true)}
        connectionsApi={{ create }}
        createConnection={createConnection}
        t={t}
        onBack={() => {}}
      />
    )

    fireEvent.click(screen.getByRole('button', { name: 'Connect' }))

    await waitFor(() => {
      expect(create).toHaveBeenCalledWith('device-1', expect.stringMatching(/^niuma-web-client-/))
      expect(createConnection).toHaveBeenCalledWith(
        expect.objectContaining({
          url: 'ws://127.0.0.1:27880/ws/client?connection_id=conn_123&connection_token=short_token'
        })
      )
    })
    expect(screen.getByText('conn_123')).not.toBeNull()
  })

  it('disables connection for an offline device and keeps the back entry available', () => {
    const onBack = vi.fn()

    render(
      <DeviceConsolePage
        device={createDevice(false)}
        connectionsApi={{ create: vi.fn() }}
        t={t}
        onBack={onBack}
      />
    )

    expect((screen.getByRole('button', { name: 'Connect' }) as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(screen.getByRole('button', { name: 'Back to devices' }))

    expect(onBack).toHaveBeenCalled()
  })
})
