import { createHash } from 'node:crypto'
import { cwd, env, exit, on } from 'node:process'

const apiUrl = env.NIUMA_LOCAL_API_URL ?? 'http://127.0.0.1:27874'
const pluginId = env.NIUMA_PLUGIN_ID ?? 'niuma-plugin-demo'
const toolId = env.NIUMA_TOOL_ID ?? 'demo_tool'
const projectPath = cwd()
const projectName = projectPath.split(/[\\/]/).filter(Boolean).at(-1) ?? 'demo-project'
const sessionId = `demo-${stableHash(projectPath)}`

// Demo 插件只上报一组稳定事件，重复启动时依靠 event id 和 dedupe_key 去重。
await postDemoEvents()

// 保持进程常驻，便于验证主程序的插件启停生命周期。
const keepAlive = setInterval(() => {}, 60_000)

for (const signal of ['SIGINT', 'SIGTERM']) {
  on(signal, () => {
    clearInterval(keepAlive)
    exit(0)
  })
}

async function postDemoEvents() {
  const now = new Date().toISOString()
  const events = [
    {
      id: `${sessionId}-started`,
      dedupe_key: `${toolId}:${sessionId}:started`,
      source: `plugin:${pluginId}`,
      tool: toolId,
      session_id: sessionId,
      project_path: projectPath,
      project_name: projectName,
      event_type: 'session_started',
      severity: 'info',
      summary: 'Demo plugin task started',
      content: 'Demo plugin reported a synthetic running state.',
      error_message: null,
      attention_resolve_key: null,
      completion_reason: null,
      failure_reason: null,
      payload_ref: null,
      created_at: now
    },
    {
      id: `${sessionId}-completed`,
      dedupe_key: `${toolId}:${sessionId}:completed`,
      source: `plugin:${pluginId}`,
      tool: toolId,
      session_id: sessionId,
      project_path: projectPath,
      project_name: projectName,
      event_type: 'assistant_message_completed',
      severity: 'info',
      summary: 'Demo plugin task completed',
      content: 'Demo plugin completed its synthetic task.',
      error_message: null,
      attention_resolve_key: null,
      completion_reason: 'normal',
      failure_reason: null,
      payload_ref: null,
      created_at: now
    }
  ]

  const response = await fetch(`${apiUrl}/api/v1/plugin-events`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ plugin_id: pluginId, events })
  })

  if (!response.ok) {
    throw new Error(`NiumaNotifier API returned HTTP ${response.status}`)
  }
  const body = await response.json()
  if (body.code !== 0) {
    throw new Error(body.message ?? 'NiumaNotifier API returned a business error')
  }
}

function stableHash(value) {
  return createHash('sha1').update(value).digest('hex').slice(0, 12)
}
