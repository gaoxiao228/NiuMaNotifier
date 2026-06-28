import { unwrapEnvelope, type ApiEnvelope } from '../shared/envelope.js'
import type { AuthStore } from '../auth/authStore.js'

export class HttpError extends Error {
  constructor(
    public status: number,
    message: string
  ) {
    super(message)
  }
}

export type HttpClient = {
  get<T>(path: string): Promise<T>
  post<T>(path: string, body?: unknown): Promise<T>
}

function joinUrl(baseUrl: string, path: string): string {
  if (!baseUrl) return path
  return `${baseUrl.replace(/\/$/, '')}/${path.replace(/^\//, '')}`
}

async function readEnvelope<T>(response: Response): Promise<ApiEnvelope<T>> {
  const text = await response.text()
  if (!text.trim()) throw new HttpError(response.status, '服务端响应为空')

  try {
    return JSON.parse(text) as ApiEnvelope<T>
  } catch {
    throw new HttpError(response.status, '服务端响应格式错误')
  }
}

export function createHttpClient(authStore: AuthStore, baseUrl = ''): HttpClient {
  async function request<T>(path: string, init: RequestInit): Promise<T> {
    const headers = new Headers(init.headers)
    const token = authStore.getToken()

    if (token) headers.set('Authorization', `Bearer ${token}`)
    if (init.body != null && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json')
    }

    let response: Response
    try {
      response = await fetch(joinUrl(baseUrl, path), {
        ...init,
        headers
      })
    } catch {
      throw new HttpError(0, '网络请求失败')
    }

    if (!response.ok) {
      throw new HttpError(response.status, `HTTP ${response.status}`)
    }

    const payload = await readEnvelope<T>(response)

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
