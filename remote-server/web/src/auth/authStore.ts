const TOKEN_KEY = 'niuma.remote.access_token'
const PROBE_KEY = 'niuma.remote.storage_probe'

export type AuthStore = {
  getToken(): string | null
  setToken(token: string): void
  clearToken(): void
}

export function createMemoryAuthStore(initialToken: string | null = null): AuthStore {
  let token = initialToken

  return {
    getToken() {
      return token
    },
    setToken(nextToken) {
      token = nextToken
    },
    clearToken() {
      token = null
    }
  }
}

function canUseStorage(storage: Storage): boolean {
  try {
    storage.setItem(PROBE_KEY, '1')
    storage.removeItem(PROBE_KEY)
    return true
  } catch {
    return false
  }
}

function resolveStorage(storage?: Storage): Storage | null {
  if (storage) return storage
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

export function createLocalStorageAuthStore(storage?: Storage): AuthStore {
  const resolvedStorage = resolveStorage(storage)
  if (!resolvedStorage || !canUseStorage(resolvedStorage)) {
    // 测试、隐私模式或 SSR 环境可能没有可用 storage，此时保持登录态仅驻留内存。
    return createMemoryAuthStore()
  }
  const memoryFallback = createMemoryAuthStore()

  return {
    getToken() {
      try {
        return resolvedStorage.getItem(TOKEN_KEY) ?? memoryFallback.getToken()
      } catch {
        return memoryFallback.getToken()
      }
    },
    setToken(token) {
      // 只保存 access token；刷新 token 后续任务再接入更完整的会话生命周期。
      memoryFallback.setToken(token)
      try {
        resolvedStorage.setItem(TOKEN_KEY, token)
      } catch {
        // storage 后续失效时继续使用内存态，避免中断 App 渲染。
      }
    },
    clearToken() {
      memoryFallback.clearToken()
      try {
        resolvedStorage.removeItem(TOKEN_KEY)
      } catch {
        // 清理失败时至少清掉内存态；持久化层异常不应阻断 UI 回到登录态。
      }
    }
  }
}
