import { act, cleanup, render, screen } from '@testing-library/react'
import type { ComponentProps } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'

import type { RemoteDevice } from '../api/devicesApi.js'
import { DeviceListPage } from '../devices/deviceListPage.js'
import { createTranslator } from '../i18n/index.js'

type Deferred<T> = {
  promise: Promise<T>
  resolve(value: T): void
}

function createDeferred<T>(): Deferred<T> {
  let resolve!: (value: T) => void
  const promise = new Promise<T>((done) => {
    resolve = done
  })
  return { promise, resolve }
}

const t = createTranslator('en')

function createDevice(id: string, name: string, online = true): RemoteDevice {
  return {
    id,
    name,
    online,
    last_seen_at: null,
    capabilities: {},
    identity_public_key: {}
  }
}

async function renderDeviceListWithDevices(
  list: RemoteDevice[],
  props: Partial<ComponentProps<typeof DeviceListPage>> = {}
) {
  const devicesApi = {
    list: vi.fn().mockResolvedValue({ list })
  }
  const result = render(
    <DeviceListPage
      devicesApi={devicesApi}
      t={t}
      {...props}
    />
  )
  await screen.findByText('Devices')
  return { ...result, devicesApi }
}

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

describe('DeviceListPage', () => {
  it('keeps the latest refresh result when an older request returns later', async () => {
    const first = createDeferred<{ list: RemoteDevice[] }>()
    const second = createDeferred<{ list: RemoteDevice[] }>()
    const firstApi = {
      list: () => first.promise
    }
    const secondApi = {
      list: () => second.promise
    }

    const { rerender } = render(
      <DeviceListPage
        devicesApi={firstApi}
        t={t}
      />
    )
    rerender(
      <DeviceListPage
        devicesApi={secondApi}
        t={t}
      />
    )

    await act(async () => {
      second.resolve({
        list: [
          {
            id: 'dev_new',
            name: 'New device',
            online: true,
            last_seen_at: null,
            capabilities: {},
            identity_public_key: {}
          }
        ]
      })
      await second.promise
    })

    await screen.findAllByText('New device')

    await act(async () => {
      first.resolve({
        list: [
          {
            id: 'dev_old',
            name: 'Old device',
            online: true,
            last_seen_at: null,
            capabilities: {},
            identity_public_key: {}
          }
        ]
      })
      await first.promise
    })

    expect(screen.getAllByText('New device').length).toBeGreaterThan(0)
    expect(screen.queryByText('Old device')).toBeNull()
  })

  it('does not request connection creation while rendering online devices', async () => {
    const fetchMock = vi.spyOn(globalThis, 'fetch').mockResolvedValue(
      new Response(
        JSON.stringify({
          code: 0,
          message: 'ok',
          data: {
            list: [createDevice('offline-1', 'Offline laptop', false), createDevice('online-1', 'Desk Mac')]
          }
        }),
        { headers: { 'Content-Type': 'application/json' } }
      )
    )
    const devicesApi = {
      list: async () => {
        const response = await fetch('/api/v1/devices/list')
        const payload = (await response.json()) as { data: { list: RemoteDevice[] } }
        return payload.data
      }
    }

    render(<DeviceListPage devicesApi={devicesApi} t={t} />)

    await screen.findByText('Desk Mac')
    const requestedUrls = fetchMock.mock.calls.map(([input]) => String(input))

    expect(requestedUrls).toContain('/api/v1/devices/list')
    expect(requestedUrls).not.toContain('/api/v1/connections/create')
  })

  it('does not expose remote connection actions on the admin device list', async () => {
    const onSelectDevice = vi.fn()
    await renderDeviceListWithDevices([
      createDevice('offline-1', 'Offline laptop', false),
      createDevice('online-1', 'Desk Mac')
    ])

    expect(screen.getAllByText('Desk Mac').length).toBeGreaterThan(0)
    expect(screen.queryByRole('columnheader', { name: 'Connect' })).toBeNull()
    expect(screen.queryByRole('button', { name: 'Connect Desk Mac' })).toBeNull()
    expect(onSelectDevice).not.toHaveBeenCalled()
  })

  it('does not render remote sessions or connection diagnostics on the device list page', async () => {
    await renderDeviceListWithDevices([createDevice('device-1', 'Desk Mac')])

    expect(screen.queryByText('Remote sessions')).toBeNull()
    expect(screen.queryByLabelText('Remote RPC status')).toBeNull()
    expect(screen.queryByText('Relay diagnostics')).toBeNull()
    expect(screen.queryByText('WebRTC diagnostics')).toBeNull()
  })
})
