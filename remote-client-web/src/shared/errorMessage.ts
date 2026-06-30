import type { MessageKey } from '../i18n/messages.js'

type Translator = (key: MessageKey) => string

type DisplayableError = {
  messageKey?: string
  message?: string
}

const apiErrorKeys = [
  'api_error_network',
  'api_error_http',
  'api_error_empty_response',
  'api_error_invalid_json'
] as const satisfies readonly MessageKey[]

function isApiErrorKey(value: string): value is (typeof apiErrorKeys)[number] {
  return apiErrorKeys.includes(value as (typeof apiErrorKeys)[number])
}

export function toDisplayErrorMessage(t: Translator, error: unknown, fallbackKey: MessageKey): string {
  if (typeof error === 'object' && error !== null) {
    const displayable = error as DisplayableError
    if (displayable.messageKey && isApiErrorKey(displayable.messageKey)) return t(displayable.messageKey)
    // 兼容部分调用方只把内部错误 key 放进 message 的情况，避免 UI 直接展示 api_error_*。
    if (displayable.message && isApiErrorKey(displayable.message)) return t(displayable.message)
    if (displayable.message) return displayable.message
  }

  return t(fallbackKey)
}
