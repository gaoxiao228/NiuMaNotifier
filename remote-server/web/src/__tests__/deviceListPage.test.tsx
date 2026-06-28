import { act, cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'

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

afterEach(() => {
  cleanup()
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

    const { rerender } = render(<DeviceListPage devicesApi={firstApi} t={t} onSelectDevice={() => {}} />)
    rerender(<DeviceListPage devicesApi={secondApi} t={t} onSelectDevice={() => {}} />)

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

    await screen.findByText('New device')

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

    expect(screen.getByText('New device')).not.toBeNull()
    expect(screen.queryByText('Old device')).toBeNull()
  })
})
