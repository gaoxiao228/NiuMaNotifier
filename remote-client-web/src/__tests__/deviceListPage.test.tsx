import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { RemoteDevice } from '../api/devicesApi.js'
import { DeviceListPage } from '../devices/deviceListPage.js'
import { I18nProvider } from '../i18n/index.js'

const devices: RemoteDevice[] = [
  {
    id: 'device-online',
    name: 'Office Mac',
    online: true,
    last_seen_at: '2026-06-30T08:00:00.000Z',
    capabilities: { screen: true, input: true },
    identity_public_key: null
  },
  {
    id: 'device-offline',
    name: 'Home PC',
    online: false,
    last_seen_at: null,
    capabilities: ['relay'],
    identity_public_key: null
  }
]

function renderDeviceList(onOpenDevice = vi.fn()) {
  const onRefresh = vi.fn()
  const onLogout = vi.fn()

  render(
    <I18nProvider>
      <DeviceListPage
        devices={devices}
        loading={false}
        userEmail="user@example.com"
        onRefresh={onRefresh}
        onLogout={onLogout}
        onOpenDevice={onOpenDevice}
      />
    </I18nProvider>
  )

  return { onOpenDevice, onRefresh, onLogout }
}

describe('DeviceListPage', () => {
  it('opens online devices and keeps offline devices disabled', () => {
    const { onOpenDevice } = renderDeviceList()

    fireEvent.click(screen.getByRole('button', { name: 'Open Office Mac' }))

    expect(onOpenDevice).toHaveBeenCalledWith(devices[0])
    expect(screen.getByRole('button', { name: 'Open Home PC' })).toBeDisabled()
  })
})
