export type DiagnosticStepStatus = 'passed' | 'failed' | 'skipped' | 'running'
export type DiagnosticSeverity = 'info' | 'warning' | 'error'
export type DiagnosticOverall = 'passed' | 'degraded' | 'failed'
export type DiagnosticScope = 'web_client'

export type DiagnosticStep = {
  key: string
  title: string
  status: DiagnosticStepStatus
  severity?: DiagnosticSeverity
  duration_ms?: number
  message?: string
  suggestion?: string
  detail?: unknown
}

export type DiagnosticReport = {
  scope: DiagnosticScope
  overall: DiagnosticOverall
  summary: string
  started_at: string
  finished_at?: string
  steps: DiagnosticStep[]
}

type StepInput = Omit<DiagnosticStep, 'status'> & {
  status?: DiagnosticStepStatus
}

const RELAY_RPC_PING_KEY = 'relay_rpc_ping'
const WEBRTC_RPC_PING_KEY = 'webrtc_rpc_ping'
const SESSION_PROJECT_GROUPS_KEY = 'session_project_groups'

export function createDiagnosticStep(input: StepInput): DiagnosticStep {
  return {
    ...input,
    status: input.status ?? 'running'
  }
}

export function startDiagnosticReport(scope: DiagnosticScope, now: Date): DiagnosticReport {
  return {
    scope,
    overall: 'degraded',
    summary: 'diagnostics_summary_running',
    started_at: now.toISOString(),
    steps: []
  }
}

function stepStatus(steps: DiagnosticStep[], key: string): DiagnosticStepStatus | undefined {
  return steps.find((step) => step.key === key)?.status
}

// Web 端诊断允许 Relay 可用但 WebRTC 不通时降级，而不是直接失败。
function calculateWebClientOverall(steps: DiagnosticStep[]): DiagnosticOverall {
  const sessionStatus = stepStatus(steps, SESSION_PROJECT_GROUPS_KEY)
  const relayStatus = stepStatus(steps, RELAY_RPC_PING_KEY)
  const webRtcStatus = stepStatus(steps, WEBRTC_RPC_PING_KEY)

  if (sessionStatus === 'failed') return 'failed'
  if (relayStatus === 'passed' && webRtcStatus === 'passed' && sessionStatus === 'passed') return 'passed'
  if (relayStatus === 'passed' && sessionStatus === 'passed') return 'degraded'
  return steps.some((step) => step.status === 'failed' && step.severity === 'error') ? 'failed' : 'degraded'
}

// summary 使用稳定的 i18n key，避免调用方重复实现诊断结果判定。
function calculateSummary(overall: DiagnosticOverall, steps: DiagnosticStep[]): string {
  if (stepStatus(steps, SESSION_PROJECT_GROUPS_KEY) === 'failed') return 'diagnostics_summary_session_failed'
  if (
    overall === 'degraded' &&
    stepStatus(steps, RELAY_RPC_PING_KEY) === 'passed' &&
    stepStatus(steps, WEBRTC_RPC_PING_KEY) === 'failed' &&
    stepStatus(steps, SESSION_PROJECT_GROUPS_KEY) === 'passed'
  ) {
    return 'diagnostics_summary_relay_available_webrtc_failed'
  }
  if (overall === 'passed') return 'diagnostics_summary_passed'
  if (overall === 'failed') return 'diagnostics_summary_failed'
  return 'diagnostics_summary_degraded'
}

export function finishDiagnosticReport(
  report: DiagnosticReport,
  steps: DiagnosticStep[],
  now: Date
): DiagnosticReport {
  const copiedSteps = [...steps]
  if (copiedSteps.some((step) => step.status === 'running')) {
    return {
      scope: report.scope,
      overall: 'degraded',
      summary: 'diagnostics_summary_running',
      started_at: report.started_at,
      steps: copiedSteps
    }
  }

  const overall = calculateWebClientOverall(copiedSteps)
  return {
    ...report,
    overall,
    summary: calculateSummary(overall, copiedSteps),
    finished_at: now.toISOString(),
    steps: copiedSteps
  }
}
