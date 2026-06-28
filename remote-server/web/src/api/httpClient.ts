import { unwrapEnvelope, type ApiEnvelope } from '../shared/envelope.js'
import type { AuthStore } from '../auth/authStore.js'

export type HttpClient = {
  get<T>(path: string): Promise<T>
  post<T>(path: string, body?: unknown): Promise<T>
}

export function createHttpClient(authStore: AuthStore, baseUrl = ''): HttpClient {
  async function request<T>(path: string, init: RequestInit): Promise<T> {
    const headers = new Headers(init.headers)
    const token = authStore.getToken()

    if (token) headers.set('Authorization', `Bearer ${token}`)
    if (init.body != null && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json')
    }

    const response = await fetch(`${baseUrl}${path}`, {
      ...init,
      headers
    })
    const payload = (await response.json()) as ApiEnvelope<T>

    // 后端业务错误统一放在 envelope 中，调用层只处理解包后的业务数据。
    return unwrapEnvelope(payload)
  }

  return {
    get(path) {
      return request(path, { method: 'GET' })
    },
    post(path, body) {
      return request(path, {
        method: 'POST',
        body: body == null ? undefined : JSON.stringify(body)
      })
    }
  }
}
