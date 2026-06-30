type Translator = (key: string) => string

type DisplayableError = {
  messageKey?: string
  message?: string
}

export function toDisplayErrorMessage(t: Translator, error: unknown, fallbackKey: string): string {
  if (typeof error === 'object' && error !== null) {
    const displayable = error as DisplayableError
    if (displayable.messageKey) return t(displayable.messageKey)
    if (displayable.message) return displayable.message
  }

  return t(fallbackKey)
}
