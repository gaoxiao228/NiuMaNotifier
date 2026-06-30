import type { LanguageCode, Translation } from './i18n'
import { escapeHtml } from './viewUtils'

export type RemoteDiagnosticStepStatus = 'passed' | 'failed' | 'skipped' | 'running'
export type RemoteDiagnosticOverall = 'passed' | 'degraded' | 'failed'
export type RemoteDiagnosticSeverity = 'info' | 'warning' | 'error'

export type RemoteDiagnosticStep = {
  key: string
  title: string
  status: RemoteDiagnosticStepStatus
  severity?: RemoteDiagnosticSeverity
  duration_ms?: number
  message?: string
  suggestion?: string
  detail?: unknown
}

export type RemoteDiagnosticReport = {
  scope: 'local_agent'
  overall: RemoteDiagnosticOverall
  summary: string
  started_at: string
  finished_at?: string
  steps: RemoteDiagnosticStep[]
}

type RenderOptions = {
  language: LanguageCode
  report: RemoteDiagnosticReport | null
  translations: Record<LanguageCode, Translation>
}

export function renderRemoteDiagnosticReport(options: RenderOptions): string {
  if (!options.report) return ''
  const t = options.translations[options.language]
  const report = options.report
  return `
    <section class="remote-diagnostics-report remote-diagnostics-${escapeHtml(report.overall)}" aria-label="${escapeHtml(
      t.remoteDiagnostics
    )}">
      <h3>${escapeHtml(t.remoteDiagnostics)}</h3>
      <p class="remote-diagnostics-summary">${escapeHtml(translateDiagnosticKey(t, report.summary))}</p>
      <dl class="remote-diagnostics-steps">
        ${report.steps.map((step) => renderStep(t, step)).join('')}
      </dl>
    </section>
  `
}

function renderStep(t: Translation, step: RemoteDiagnosticStep): string {
  const message = step.message ? `<span>${escapeHtml(translateDiagnosticKey(t, step.message))}</span>` : ''
  const duration = typeof step.duration_ms === 'number' ? `<span>${escapeHtml(`${step.duration_ms}ms`)}</span>` : ''
  return `
    <div class="remote-diagnostics-step">
      <dt>${escapeHtml(translateDiagnosticKey(t, step.title))}</dt>
      <dd>
        <span class="remote-diagnostics-status remote-diagnostics-status-${escapeHtml(step.status)}">${escapeHtml(
          t.remoteDiagnosticsStatus[step.status] ?? step.status
        )}</span>
        ${duration}
        ${message}
      </dd>
    </div>
  `
}

function translateDiagnosticKey(t: Translation, key: string): string {
  return t.remoteDiagnosticsMessages[key] ?? key
}
