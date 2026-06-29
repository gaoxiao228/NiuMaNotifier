import { describe, expect, it } from 'vitest'
import { planDeviceDedupe } from '../scripts/dedupe-devices.js'

describe('devices dedupe script', () => {
  it('keeps latest active device per name', () => {
    const groups = planDeviceDedupe([
      { id: 'dev_old', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' },
      { id: 'dev_new', name: 'MacBook Pro', created_at: '2026-06-02T00:00:00.000Z' },
      { id: 'dev_mid', name: 'MacBook Pro', created_at: '2026-06-01T12:00:00.000Z' }
    ])

    expect(groups).toEqual([
      {
        name: 'MacBook Pro',
        keep: { id: 'dev_new', name: 'MacBook Pro', created_at: '2026-06-02T00:00:00.000Z' },
        revoke: [
          { id: 'dev_mid', name: 'MacBook Pro', created_at: '2026-06-01T12:00:00.000Z' },
          { id: 'dev_old', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' }
        ]
      }
    ])
  })

  it('does not revoke unique names', () => {
    const groups = planDeviceDedupe([
      { id: 'dev_1', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' },
      { id: 'dev_2', name: 'iMac', created_at: '2026-06-02T00:00:00.000Z' }
    ])

    expect(groups).toEqual([])
  })

  it('uses ascending id as stable tie-breaker when created_at is equal', () => {
    const groups = planDeviceDedupe([
      { id: 'dev_c', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' },
      { id: 'dev_a', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' },
      { id: 'dev_b', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' }
    ])

    expect(groups).toEqual([
      {
        name: 'MacBook Pro',
        keep: { id: 'dev_a', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' },
        revoke: [
          { id: 'dev_b', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' },
          { id: 'dev_c', name: 'MacBook Pro', created_at: '2026-06-01T00:00:00.000Z' }
        ]
      }
    ])
  })
})
