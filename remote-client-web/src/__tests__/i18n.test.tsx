import { fireEvent, render, screen } from '@testing-library/react'
import { beforeEach, describe, expect, it } from 'vitest'

import { I18nProvider, useI18n } from '../i18n/index.js'

function installLocalStorageMock() {
  const values = new Map<string, string>()
  Object.defineProperty(window, 'localStorage', {
    configurable: true,
    value: {
      clear: () => values.clear(),
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => values.set(key, value),
      removeItem: (key: string) => values.delete(key)
    }
  })
}

function LanguageProbe() {
  const { language, setLanguage, t } = useI18n()

  return (
    <div>
      <span data-testid="language">{language}</span>
      <span>{t('login_button')}</span>
      <button type="button" onClick={() => setLanguage('ja')}>ja</button>
    </div>
  )
}

describe('I18nProvider', () => {
  beforeEach(() => {
    installLocalStorageMock()
    window.localStorage.clear()
  })

  it('persists manually selected language', () => {
    const { unmount } = render(
      <I18nProvider>
        <LanguageProbe />
      </I18nProvider>
    )

    fireEvent.click(screen.getByRole('button', { name: 'ja' }))

    expect(screen.getByTestId('language')).toHaveTextContent('ja')
    expect(screen.getByText('サインイン')).toBeInTheDocument()

    unmount()
    render(
      <I18nProvider>
        <LanguageProbe />
      </I18nProvider>
    )

    expect(screen.getByTestId('language')).toHaveTextContent('ja')
  })
})
