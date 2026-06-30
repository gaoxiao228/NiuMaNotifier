import {
  createDiagnosticStep,
  finishDiagnosticReport,
  startDiagnosticReport,
  type DiagnosticStep
} from '../remote/diagnostics.js'

describe('remote diagnostics helpers', () => {
  it('creates a running diagnostic step by default', () => {
    expect(createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping' })).toEqual({
      key: 'relay_rpc_ping',
      title: 'Relay Ping',
      status: 'running'
    })
  })

  it('starts a web client diagnostic report with initial fields', () => {
    expect(startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))).toEqual({
      scope: 'web_client',
      overall: 'degraded',
      summary: 'diagnostics_summary_running',
      started_at: '2026-06-30T00:00:00.000Z',
      steps: []
    })
  })

  it('marks all passed web client checks as passed', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({ key: 'webrtc_rpc_ping', title: 'WebRTC Ping', status: 'passed' }),
      createDiagnosticStep({ key: 'session_project_groups', title: 'Sessions', status: 'passed' })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('passed')
    expect(finished.summary).toBe('diagnostics_summary_passed')
  })

  it('marks relay fallback as degraded instead of failed', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({
        key: 'webrtc_rpc_ping',
        title: 'WebRTC Ping',
        status: 'failed',
        severity: 'warning',
        message: 'WebRTC ping timeout'
      }),
      createDiagnosticStep({ key: 'session_project_groups', title: 'Sessions', status: 'passed' })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('degraded')
    expect(finished.summary).toBe('diagnostics_summary_relay_available_webrtc_failed')
  })

  it('does not use relay fallback summary while session check is skipped', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({
        key: 'webrtc_rpc_ping',
        title: 'WebRTC Ping',
        status: 'failed',
        severity: 'warning',
        message: 'WebRTC ping timeout'
      }),
      createDiagnosticStep({ key: 'session_project_groups', title: 'Sessions', status: 'skipped' })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('degraded')
    expect(finished.summary).toBe('diagnostics_summary_degraded')
  })

  it('keeps a report running while any step is still running', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({ key: 'webrtc_rpc_ping', title: 'WebRTC Ping', status: 'running' }),
      createDiagnosticStep({ key: 'session_project_groups', title: 'Sessions', status: 'passed' })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('degraded')
    expect(finished.summary).toBe('diagnostics_summary_running')
    expect(finished.finished_at).toBeUndefined()
  })

  it('marks session read failure as failed even when transports work', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({ key: 'webrtc_rpc_ping', title: 'WebRTC Ping', status: 'passed' }),
      createDiagnosticStep({
        key: 'session_project_groups',
        title: 'Sessions',
        status: 'failed',
        severity: 'error',
        message: 'remote_sessions_failed'
      })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('failed')
    expect(finished.summary).toBe('diagnostics_summary_session_failed')
  })

  it('marks an error failed step as failed without available session fallback', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'failed', severity: 'error' }),
      createDiagnosticStep({ key: 'webrtc_rpc_ping', title: 'WebRTC Ping', status: 'passed' })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))

    expect(finished.overall).toBe('failed')
    expect(finished.summary).toBe('diagnostics_summary_failed')
  })

  it('copies steps so later array mutation does not change a returned report', () => {
    const report = startDiagnosticReport('web_client', new Date('2026-06-30T00:00:00.000Z'))
    const steps: DiagnosticStep[] = [
      createDiagnosticStep({ key: 'relay_rpc_ping', title: 'Relay Ping', status: 'passed' }),
      createDiagnosticStep({ key: 'webrtc_rpc_ping', title: 'WebRTC Ping', status: 'passed' }),
      createDiagnosticStep({ key: 'session_project_groups', title: 'Sessions', status: 'passed' })
    ]

    const finished = finishDiagnosticReport(report, steps, new Date('2026-06-30T00:00:01.000Z'))
    steps.push(createDiagnosticStep({ key: 'extra_check', title: 'Extra Check', status: 'failed' }))

    expect(finished.steps).toHaveLength(3)
  })
})
