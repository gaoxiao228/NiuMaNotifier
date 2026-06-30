import { describe, expect, it } from 'vitest'

import {
  isProjectGroupPage,
  sessionDescription,
  sessionDisplayStatus,
  sessionTitle
} from '../remote/remoteSessionTypes.js'

describe('remote session types', () => {
  it('accepts a valid session project group page', () => {
    expect(
      isProjectGroupPage({
        list: [
          {
            tool: 'codex',
            project_name: 'repo',
            project_path: '/repo',
            sessions: [
              {
                normalized_session_id: 'normalized-1',
                primary_session_id: 'primary-1',
                title: 'Demo session',
                runtime_status: 'running',
                status: 'idle',
                first_user_message_preview: 'Inspect work',
                latest_event_summary: null,
                subagent_count: 0
              }
            ]
          }
        ],
        page: 1,
        page_size: 20,
        total: 1
      })
    ).toBe(true)
  })

  it('rejects invalid session group shapes', () => {
    expect(isProjectGroupPage({ list: [{ sessions: 'not-array' }] })).toBe(false)
    expect(isProjectGroupPage({ list: [{ sessions: [{ subagent_count: '0' }] }] })).toBe(false)
  })

  it('derives display text from runtime status and fallbacks', () => {
    const session = {
      normalized_session_id: 'normalized-1',
      primary_session_id: 'primary-1',
      runtime_status: null,
      status: 'active',
      first_user_message_preview: '',
      latest_event_summary: 'Latest event'
    }
    expect(sessionDisplayStatus(session)).toBe('active')
    expect(sessionTitle(session)).toBe('primary-1')
    expect(sessionDescription(session)).toBe('Latest event')
  })
})
