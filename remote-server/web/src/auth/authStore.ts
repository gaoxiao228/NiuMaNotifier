const TOKEN_KEY = 'niuma.remote.access_token'

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

function resolveStorage(storage?: Storage): Storage | null {
  if (storage) return storage
  if (typeof window === 'undefined') return null

  try {
    return window.localStorage
  } catch {
    return null
  }
}

export function createLocalStorageAuthStore(storage?: Storage): AuthStore {
  const resolvedStorage = resolveStorage(storage)
  if (!resolvedStorage) {
    // 测试、隐私模式或 SSR 环境可能没有可用 localStorage，此时保持登录态仅驻留内存。
    return createMemoryAuthStore()
  }

  return {
    getToken() {
      return resolvedStorage.getItem(TOKEN_KEY)
    },
    setToken(token) {
      // 只保存 access token；刷新 token 后续任务再接入更完整的会话生命周期。
      resolvedStorage.setItem(TOKEN_KEY, token)
    },
    clearToken() {
      resolvedStorage.removeItem(TOKEN_KEY)
    }
  }
}
