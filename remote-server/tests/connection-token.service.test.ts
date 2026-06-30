import { describe, expect, it } from 'vitest'
import { createConnectionTokenService } from '../src/modules/connections/connection-token.service.js'

describe('connection token service', () => {
  it('creates opaque connection tokens and verifies hashes', () => {
    const service = createConnectionTokenService({ tokenPepper: 'pepper' })
    const issued = service.issue()

    expect(issued.token).toMatch(/^cnt_[A-Za-z0-9_-]{43,}$/)
    expect(issued.tokenHash).toHaveLength(64)
    expect(service.verify(issued.token, issued.tokenHash)).toBe(true)
    expect(service.verify('cnt_wrong', issued.tokenHash)).toBe(false)
  })
})
