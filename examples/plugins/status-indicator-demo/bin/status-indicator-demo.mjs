import process, { env, exit } from 'node:process'

const apiUrl = env.NIUMA_LOCAL_API_URL ?? 'http://127.0.0.1:27874'
const pluginId = env.NIUMA_PLUGIN_ID ?? 'status-indicator-demo'
const reconnectMs = 3_000

void runStateStreamLoop().catch((error) => {
  console.error(error)
  exit(1)
})

for (const signal of ['SIGINT', 'SIGTERM']) {
  process.on(signal, () => exit(0))
}

async function runStateStreamLoop() {
  while (true) {
    try {
      await consumeStateStream()
    } catch (error) {
      console.error(`[${pluginId}] state stream disconnected:`, error.message ?? error)
    }
    await delay(reconnectMs)
  }
}

async function consumeStateStream() {
  // 状态指示插件只消费主状态流，不上报事件也不修改主程序状态。
  const response = await fetch(`${apiUrl}/api/v1/state/stream`, {
    headers: { Accept: 'text/event-stream' }
  })
  if (!response.ok || !response.body) {
    throw new Error(`NiumaNotifier state stream returned HTTP ${response.status}`)
  }

  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader()
  let buffer = ''
  while (true) {
    const { value, done } = await reader.read()
    if (done) {
      break
    }
    buffer += value
    const frames = buffer.split(/\r?\n\r?\n/)
    buffer = frames.pop() ?? ''
    for (const frame of frames) {
      dispatchSseFrame(frame)
    }
  }
}

function dispatchSseFrame(frame) {
  const event = { name: 'message', data: [] }
  for (const line of frame.split(/\r?\n/)) {
    if (!line || line.startsWith(':')) {
      continue
    }
    const separator = line.indexOf(':')
    const field = separator >= 0 ? line.slice(0, separator) : line
    const value = separator >= 0 ? line.slice(separator + 1).replace(/^ /, '') : ''
    if (field === 'event') {
      event.name = value
    } else if (field === 'data') {
      event.data.push(value)
    }
  }

  if (event.name !== 'state' || event.data.length === 0) {
    return
  }
  const state = JSON.parse(event.data.join('\n'))
  renderIndicatorState(state)
}

function renderIndicatorState(state) {
  const view = indicatorViewForStatus(state.status)
  const project = state.session?.project_name ?? 'no project'
  const summary = state.detail?.summary ?? ''
  console.log(
    `[${pluginId}] ${view.icon} ${view.label} | ${project}${summary ? ` | ${summary}` : ''}`
  )
}

function indicatorViewForStatus(status) {
  switch (status) {
    case 'running':
      return { icon: 'BLUE', label: 'working' }
    case 'waiting_approval':
      return { icon: 'YELLOW', label: 'approval required' }
    case 'waiting_input':
      return { icon: 'YELLOW', label: 'input required' }
    case 'completed':
      return { icon: 'GREEN', label: 'completed' }
    case 'error':
      return { icon: 'RED', label: 'error' }
    default:
      return { icon: 'GRAY', label: 'idle' }
  }
}

function delay(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms))
}
