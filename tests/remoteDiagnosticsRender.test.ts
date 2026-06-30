import { renderRemoteDiagnosticReport, type RemoteDiagnosticReport } from '../src/remoteDiagnostics'
import { translations } from '../src/i18n'

const report: RemoteDiagnosticReport = {
  scope: 'local_agent',
  overall: 'degraded',
  summary: 'remoteDiagnosticsSummaryDegraded',
  started_at: '2026-06-30T00:00:00.000Z',
  finished_at: '2026-06-30T00:00:01.000Z',
  steps: [
    {
      key: 'binding',
      title: 'remoteDiagnosticsStepBinding',
      status: 'passed',
      duration_ms: 2
    },
    {
      key: 'device_socket',
      title: 'remoteDiagnosticsStepDeviceSocket',
      status: 'failed',
      severity: 'warning',
      message: 'remoteDiagnosticsMessageServerUnreachable',
      duration_ms: 10
    }
  ]
}

const html = renderRemoteDiagnosticReport({
  language: 'zh-CN',
  report,
  translations
})

if (!html.includes('远程访问诊断')) {
  throw new Error('诊断报告应显示标题')
}

if (!html.includes('远程访问部分可用，但存在需要处理的项目')) {
  throw new Error('诊断报告应显示摘要')
}

if (!html.includes('账号与设备绑定') || !html.includes('设备信令连接')) {
  throw new Error('诊断报告应显示步骤标题')
}

if (!html.includes('通过') || !html.includes('失败')) {
  throw new Error('诊断报告应显示步骤状态')
}

if (!html.includes('无法连接远程服务端')) {
  throw new Error('诊断报告应显示失败原因')
}

if (html.includes('<script>')) {
  throw new Error('诊断报告必须转义动态内容')
}
