import { describe, expect, it } from 'vitest'

import { resolveRemoteServerUrl } from '../App.js'

describe('resolveRemoteServerUrl', () => {
  it('falls back to the browser origin when no Vite server URL is configured', () => {
    expect(resolveRemoteServerUrl()).toBe(window.location.origin)
  })
})
