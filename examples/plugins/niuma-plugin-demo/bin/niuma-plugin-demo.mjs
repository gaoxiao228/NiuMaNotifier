import { createHash } from 'node:crypto'
import process, { cwd, env, exit } from 'node:process'

const apiUrl = env.NIUMA_LOCAL_API_URL ?? 'http://127.0.0.1:27874'
const pluginId = env.NIUMA_PLUGIN_ID ?? 'niuma-plugin-demo'
const toolId = env.NIUMA_TOOL_ID ?? 'demo_tool'
const projectPath = cwd()
const projectName = projectPath.split(/[\\/]/).filter(Boolean).at(-1) ?? 'demo-project'
const runId = `${Date.now()}-${stableHash(`${projectPath}:${Math.random()}`)}`
const sessionId = `demo-${stableHash(projectPath)}-${runId}`

// Demo 插件按固定时间线模拟工具状态，便于观察主程序的状态流转。
void runDemoTimeline().catch((error) => {
  console.error(error)
  exit(1)
})

// 保持进程常驻，便于验证主程序的插件启停生命周期。
const keepAlive = setInterval(() => {}, 60_000)

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => {
    clearInterval(keepAlive)
    exit(0)
  })
}

async function runDemoTimeline() {
  await delay(1_000)
  await postDemoEvent({
    step: 'running',
    event_type: 'session_started',
    severity: 'info',
    summary: 'Demo plugin task started',
    content: 'Demo plugin reported a synthetic running state.'
  })

  await delay(5_000)
  await postDemoEvent({
    step: 'failed',
    event_type: 'task_failed',
    severity: 'urgent',
    summary: 'Demo plugin simulated an error',
    content: 'Demo plugin emitted a synthetic error event.',
    error_message: 'Demo plugin simulated error',
    failure_reason: 'unknown'
  })

  await delay(5_000)
  await postDemoEvent({
    step: 'approval',
    event_type: 'approval_requested',
    severity: 'urgent',
    summary: 'Demo plugin requests approval',
    content: 'Demo plugin is asking for a synthetic approval.'
  })

  await delay(5_000)
  await postDemoEvent({
    step: 'completed',
    event_type: 'assistant_message_completed',
    severity: 'info',
    summary: 'Demo plugin task completed',
    content: 'Demo plugin completed its synthetic task.',
    completion_reason: 'normal'
  })
}

async function postDemoEvent(event) {
  const payload = {
    id: `${sessionId}-${event.step}`,
    dedupe_key: `${toolId}:${sessionId}:${event.step}`,
    source: `plugin:${pluginId}`,
    tool: toolId,
    session_id: sessionId,
    project_path: projectPath,
    project_name: projectName,
    event_type: event.event_type,
    severity: event.severity,
    summary: event.summary,
    content: event.content,
    error_message: event.error_message ?? null,
    attention_resolve_key: event.attention_resolve_key ?? null,
    completion_reason: event.completion_reason ?? null,
    failure_reason: event.failure_reason ?? null,
    payload_ref: null,
    created_at: new Date().toISOString()
  }

  const response = await fetch(`${apiUrl}/api/v1/plugin-events`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ plugin_id: pluginId, events: [payload] })
  })

  if (!response.ok) {
    throw new Error(`NiumaNotifier API returned HTTP ${response.status}`)
  }
  const body = await response.json()
  if (body.code !== 0) {
    throw new Error(body.message ?? 'NiumaNotifier API returned a business error')
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}

function stableHash(value) {
  return createHash('sha1').update(value).digest('hex').slice(0, 12)
}
