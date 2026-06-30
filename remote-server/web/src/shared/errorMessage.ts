type Translator = (key: string) => string

type KeyedError = {
  messageKey?: string
  message?: string
}

export function toDisplayErrorMessage(t: Translator, error: unknown, fallbackKey: string): string {
  if (typeof error === 'object' && error !== null) {
    const keyedError = error as KeyedError
    if (keyedError.messageKey) return t(keyedError.messageKey)
    if (keyedError.message) return keyedError.message
  }

  return t(fallbackKey)
}
