import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import type { RemoteDevice } from '../api/devicesApi.js'
import { I18nProvider } from '../i18n/index.js'
import type { RemoteDeviceSessionSnapshot } from '../remote/remoteDeviceSessionController.js'
import { SessionConsolePage } from '../sessions/sessionConsolePage.js'

const device: RemoteDevice = {
  id: 'device-1',
  name: 'Office Mac',
  online: true,
  last_seen_at: '2026-06-30T08:00:00.000Z',
  capabilities: {},
  identity_public_key: null
}

const snapshot: RemoteDeviceSessionSnapshot = {
  connectionStatus: 'accepted',
  relayStatus: 'open',
  webRtcStatus: 'connecting',
  activeTransport: 'relay',
  connectionId: 'conn-1',
  error: null,
  pingResult: { status: 'idle', value: null },
  stateResult: { status: 'idle', value: null },
  sessionsResult: {
    status: 'ready',
    value: {
      list: [
        {
          tool: 'codex',
          project_path: '/Users/niuma/code/NiuMaNotifier',
          sessions: [
            {
              primary_session_id: 'session-1',
              title: 'Build Remote Session List Page',
              status: 'running',
              updated_at: '2026-06-30T09:00:00.000Z'
            }
          ]
        }
      ]
    }
  },
  diagnostics: {
    relay: { status: 'idle', value: null },
    webrtc: { status: 'idle', value: null }
  },
  diagnosticReport: null,
  diagnosticRunning: false
}

describe('SessionConsolePage', () => {
  it('shows device, transport states, active channel, and sessions', () => {
    const onBack = vi.fn()

    render(
      <I18nProvider>
        <SessionConsolePage device={device} snapshot={snapshot} onBack={onBack} onLogout={vi.fn()} />
      </I18nProvider>
    )

    expect(screen.getByRole('heading', { name: 'Office Mac' })).toBeInTheDocument()
    expect(screen.getByText('Connection')).toBeInTheDocument()
    expect(screen.getByText('Accepted')).toBeInTheDocument()
    expect(screen.getAllByText('Relay')).toHaveLength(2)
    expect(screen.getByText('Open')).toBeInTheDocument()
    expect(screen.getByText('WebRTC')).toBeInTheDocument()
    expect(screen.getByText('Connecting')).toBeInTheDocument()
    expect(screen.getByText('Active channel')).toBeInTheDocument()
    expect(screen.getByText('Build Remote Session List Page')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: 'Back to devices' }))
    expect(onBack).toHaveBeenCalledTimes(1)
  })
})
