import type { ZodError, ZodSchema } from 'zod'
import { ErrorCode } from './errors.js'
import { apiFailure, type ApiEnvelope } from './response.js'

export function formatZodError(error: ZodError): string {
  return error.issues
    .map((issue) => `${issue.path.join('.') || 'body'}${issue.message}`)
    .join('；')
}

export function parseBody<T>(
  schema: ZodSchema<T>,
  value: unknown
): { ok: true; data: T } | { ok: false; response: ApiEnvelope } {
  const parsed = schema.safeParse(value)
  if (parsed.success) return { ok: true, data: parsed.data }

  // 字段校验错误按接口规范合并到外层 message。
  return {
    ok: false,
    response: apiFailure(ErrorCode.BUSINESS_VALIDATION_FAILED, formatZodError(parsed.error))
  }
}
