export type ApiEnvelope<T> = {
  code: number
  message: string
  data: T | null
}

export class ApiError extends Error {
  constructor(
    public code: number,
    message: string,
    public messageKey?: string
  ) {
    super(message)
  }
}

export function unwrapEnvelope<T>(payload: ApiEnvelope<T>): T {
  if (payload.code !== 0) throw new ApiError(payload.code, payload.message)
  // code=0 必须携带 data，避免调用方把空成功响应当作有效业务数据。
  if (payload.data == null) throw new ApiError(900001, 'api_error_missing_data', 'api_error_missing_data')
  return payload.data
}
