import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { I18nProvider } from '../i18n/index.js'
import { SessionGroupsView } from '../sessions/SessionGroupsView.js'
import type { RemoteSessionProjectGroupPage } from '../remote/remoteSessionTypes.js'

function renderView(page: RemoteSessionProjectGroupPage) {
  render(
    <I18nProvider>
      <SessionGroupsView page={page} loading={false} error={null} />
    </I18nProvider>
  )
}

describe('SessionGroupsView', () => {
  it('shows project groups and localized session statuses', () => {
    renderView({
      list: [
        {
          tool: 'codex',
          project_path: '/Users/niuma/code/NiuMaNotifier',
          sessions: [
            {
              primary_session_id: 's-running',
              title: '实现远程列表',
              status: 'running',
              updated_at: '2026-06-30T08:00:00.000Z'
            },
            {
              primary_session_id: 's-waiting-input',
              title: '等待用户输入',
              status: 'waiting_input',
              updated_at: '2026-06-30T08:01:00.000Z'
            },
            {
              primary_session_id: 's-waiting-approval',
              title: '等待审批',
              status: 'waiting_approval',
              updated_at: '2026-06-30T08:02:00.000Z'
            },
            {
              primary_session_id: 's-error',
              title: '失败会话',
              status: 'error',
              updated_at: '2026-06-30T08:03:00.000Z'
            },
            {
              primary_session_id: 's-completed',
              title: '完成会话',
              status: 'completed',
              updated_at: '2026-06-30T08:04:00.000Z'
            }
          ]
        }
      ]
    })

    expect(screen.getAllByText('/Users/niuma/code/NiuMaNotifier')).toHaveLength(5)
    expect(screen.getAllByText('codex')).toHaveLength(5)
    expect(screen.getByText('实现远程列表')).toBeInTheDocument()
    expect(screen.getByText('Running')).toBeInTheDocument()
    expect(screen.getByText('Waiting for input')).toBeInTheDocument()
    expect(screen.getByText('Waiting for approval')).toBeInTheDocument()
    expect(screen.getByText('Error')).toBeInTheDocument()
    expect(screen.getByText('Completed')).toBeInTheDocument()
  })
})
