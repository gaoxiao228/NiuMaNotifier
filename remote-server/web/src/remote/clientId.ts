const CLIENT_ID_KEY = 'niuma.remote.client_id'
let memoryClientId: string | null = null

function randomClientId(): string {
  const randomPart =
    typeof crypto !== 'undefined' && 'randomUUID' in crypto
      ? crypto.randomUUID()
      : Math.random().toString(36).slice(2)
  return `niuma-web-client-${randomPart}`
}

function resolveClientStorage(): Storage | null {
  if (typeof window === 'undefined') return null

  try {
    const descriptor = Object.getOwnPropertyDescriptor(window, 'localStorage')
    if (descriptor && 'value' in descriptor) return descriptor.value as Storage
    if (typeof process !== 'undefined' && process.versions?.node) return null
    if (window.location.protocol === 'about:') return null
    return window.localStorage
  } catch {
    return null
  }
}

export function getStableClientId(): string {
  const fallback = memoryClientId ?? randomClientId()
  memoryClientId = fallback
  const storage = resolveClientStorage()
  if (!storage) return fallback

  try {
    const current = storage.getItem(CLIENT_ID_KEY)
    if (current) return current
    storage.setItem(CLIENT_ID_KEY, fallback)
  } catch {
    // 浏览器禁用 storage 时仍允许本次页面会话继续发起连接。
  }
  return fallback
}
