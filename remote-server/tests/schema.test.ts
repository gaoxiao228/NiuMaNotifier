import { describe, expect, it } from 'vitest'
import {
  desktopLoginSessions,
  devices,
  refreshTokens,
  remoteConnections,
  users
} from '../src/db/schema.js'

describe('drizzle schema', () => {
  it('exports core tables', () => {
    expect(users).toBeDefined()
    expect(refreshTokens).toBeDefined()
    expect(devices).toBeDefined()
    expect(remoteConnections).toBeDefined()
    expect(desktopLoginSessions).toBeDefined()
  })
})
