import { unwrapEnvelope, type ApiEnvelope } from '../shared/envelope.js'
import type { AuthStore } from '../auth/authStore.js'

export class HttpError extends Error {
  constructor(
    public status: number,
    public messageKey: string
  ) {
    super(messageKey)
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

function parseEnvelope<T>(text: string): ApiEnvelope<T> | null {
  try {
    const payload = JSON.parse(text) as Partial<ApiEnvelope<T>>
    if (typeof payload.code !== 'number' || typeof payload.message !== 'string' || !('data' in payload)) {
      return null
    }
    return payload as ApiEnvelope<T>
  } catch {
    return null
  }
}

async function readEnvelope<T>(response: Response): Promise<ApiEnvelope<T>> {
  const text = await response.text()
  if (!text.trim()) throw new HttpError(response.status, 'api_error_empty_response')

  const envelope = parseEnvelope<T>(text)
  if (!envelope) throw new HttpError(response.status, 'api_error_invalid_json')
  return envelope
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
      throw new HttpError(0, 'api_error_network')
    }

    if (!response.ok) {
      const text = await response.text()
      const envelope = text.trim() ? parseEnvelope<T>(text) : null
      if (envelope && envelope.code !== 0) return unwrapEnvelope(envelope)
      throw new HttpError(response.status, 'api_error_http')
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
