import type { ErrorCodeValue } from './errors.js'

export type ApiEnvelope<T extends object = Record<string, unknown>> = {
  code: number
  message: string
  data: T | null
}

export function apiSuccess<T extends object>(data: T): ApiEnvelope<T> {
  return { code: 0, message: 'ok', data }
}

export function apiFailure(
  code: ErrorCodeValue,
  message: string,
  data: Record<string, unknown> | null = null
): ApiEnvelope {
  return { code, message, data }
}
