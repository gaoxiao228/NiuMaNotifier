const DEFAULT_STORAGE_KEY = 'niuma.remote.client.session'
const PROBE_KEY = 'niuma.remote.client.storage_probe'

export type AuthUser = {
  id: string
  email: string
  role: 'admin' | 'user'
  status: 'active' | 'disabled'
}

export type AuthSession = {
  accessToken: string | null
  refreshToken: string | null
  user: AuthUser | null
}

export type AuthStore = {
  getSnapshot(): AuthSession
  setSession(session: Required<AuthSession>): void
  clear(): void
  subscribe(listener: (session: AuthSession) => void): () => void
}

const EMPTY_SESSION: AuthSession = {
  accessToken: null,
  refreshToken: null,
  user: null
}

function cloneSession(session: AuthSession): AuthSession {
  return {
    accessToken: session.accessToken,
    refreshToken: session.refreshToken,
    user: session.user ? { ...session.user } : null
  }
}

function isAuthUser(value: unknown): value is AuthUser {
  if (typeof value !== 'object' || value === null) return false
  const user = value as Partial<AuthUser>
  return (
    typeof user.id === 'string' &&
    typeof user.email === 'string' &&
    (user.role === 'admin' || user.role === 'user') &&
    (user.status === 'active' || user.status === 'disabled')
  )
}

function isStoredSession(value: unknown): value is Required<AuthSession> {
  if (typeof value !== 'object' || value === null) return false
  const session = value as Partial<AuthSession>
  return (
    typeof session.accessToken === 'string' &&
    typeof session.refreshToken === 'string' &&
    isAuthUser(session.user)
  )
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

function resolveStorage(): Storage | null {
  if (typeof window === 'undefined') return null
  try {
    return window.localStorage
  } catch {
    return null
  }
}

function readSession(storage: Storage | null, storageKey: string): AuthSession {
  if (!storage) return cloneSession(EMPTY_SESSION)
  try {
    const raw = storage.getItem(storageKey)
    if (!raw) return cloneSession(EMPTY_SESSION)
    const parsed = JSON.parse(raw) as unknown
    if (isStoredSession(parsed)) return cloneSession(parsed)
    storage.removeItem(storageKey)
    return cloneSession(EMPTY_SESSION)
  } catch {
    // 损坏 JSON 或 storage 读取异常都不应阻断应用启动，统一降级为空会话。
    try {
      storage.removeItem(storageKey)
    } catch {
      // 忽略二次清理失败，调用方仍然拿到空会话。
    }
    return cloneSession(EMPTY_SESSION)
  }
}

export function createAuthStore(storageKey = DEFAULT_STORAGE_KEY): AuthStore {
  const resolvedStorage = resolveStorage()
  const storage = resolvedStorage && canUseStorage(resolvedStorage) ? resolvedStorage : null
  const listeners = new Set<(session: AuthSession) => void>()
  let session = readSession(storage, storageKey)

  function notify() {
    const snapshot = cloneSession(session)
    for (const listener of listeners) listener(snapshot)
  }

  function persist(nextSession: AuthSession) {
    if (!storage) return
    try {
      if (nextSession.accessToken && nextSession.refreshToken && nextSession.user) {
        storage.setItem(storageKey, JSON.stringify(nextSession))
      } else {
        storage.removeItem(storageKey)
      }
    } catch {
      // localStorage 配额或权限变化时保留内存态，避免登录流程被持久化层打断。
    }
  }

  return {
    getSnapshot() {
      return cloneSession(session)
    },
    setSession(nextSession) {
      session = cloneSession(nextSession)
      persist(session)
      notify()
    },
    clear() {
      session = cloneSession(EMPTY_SESSION)
      persist(session)
      notify()
    },
    subscribe(listener) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    }
  }
}
