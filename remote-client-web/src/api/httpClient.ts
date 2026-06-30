export type ApiEnvelope<T> = {
  code: number
  message: string
  data: T
}

export class ApiError extends Error {
  constructor(
    public code: number,
    message: string,
    public status: number,
    public messageKey?: string
  ) {
    super(message)
    this.name = 'ApiError'
  }
}

export type HttpClient = {
  get<T>(path: string): Promise<T>
  post<T>(path: string, body?: unknown): Promise<T>
}

export type HttpClientOptions = {
  baseUrl: string
  getAccessToken?: () => string | null
  onAuthExpired?: () => void
}

const AUTH_EXPIRED_CODES = new Set([200001, 200002, 200003])

function joinUrl(baseUrl: string, path: string): string {
  if (!baseUrl) return path
  return `${baseUrl.replace(/\/+$/, '')}/${path.replace(/^\/+/, '')}`
}

function parseEnvelope<T>(text: string): ApiEnvelope<T> | null {
  try {
    const value = JSON.parse(text) as Partial<ApiEnvelope<T>>
    if (typeof value.code !== 'number' || typeof value.message !== 'string' || !('data' in value)) {
      return null
    }
    return value as ApiEnvelope<T>
  } catch {
    return null
  }
}

function createClientError(code: number, message: string, status: number, messageKey?: string): ApiError {
  return new ApiError(code, message, status, messageKey)
}

export function createHttpClient(options: HttpClientOptions): HttpClient {
  async function request<T>(path: string, init: RequestInit): Promise<T> {
    const headers = new Headers(init.headers)
    const token = options.getAccessToken?.()

    if (token) headers.set('Authorization', `Bearer ${token}`)
    if (init.body != null && !headers.has('Content-Type')) {
      headers.set('Content-Type', 'application/json')
    }

    let response: Response
    try {
      response = await fetch(joinUrl(options.baseUrl, path), {
        ...init,
        headers
      })
    } catch {
      // fetch 会抛出 TypeError 等浏览器原始错误，统一转换给调用层处理。
      throw createClientError(-1, 'api_error_network', 0, 'api_error_network')
    }

    const text = await response.text()
    const envelope = text.trim() ? parseEnvelope<T>(text) : null

    if (!response.ok) {
      if (envelope) throwEnvelopeError(envelope, response.status, options.onAuthExpired)
      throw createClientError(response.status, 'api_error_http', response.status, 'api_error_http')
    }

    if (!envelope) {
      const messageKey = text.trim() ? 'api_error_invalid_json' : 'api_error_empty_response'
      throw createClientError(-1, messageKey, response.status, messageKey)
    }

    if (envelope.code !== 0) {
      throwEnvelopeError(envelope, response.status, options.onAuthExpired)
    }

    return envelope.data
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

function throwEnvelopeError<T>(envelope: ApiEnvelope<T>, status: number, onAuthExpired?: () => void): never {
  if (AUTH_EXPIRED_CODES.has(envelope.code)) {
    onAuthExpired?.()
  }
  throw createClientError(envelope.code, envelope.message, status)
}
