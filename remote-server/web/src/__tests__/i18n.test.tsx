import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { App } from '../App.js'
import { messages, supportedLanguages } from '../i18n/messages.js'

afterEach(() => {
  cleanup()
})

describe('i18n messages', () => {
  it('includes console chrome labels for every supported language', () => {
    for (const language of supportedLanguages) {
      expect(messages[language].language).toBeTruthy()
      expect(messages[language].identifier).toBeTruthy()
    }
  })

  it('sets the document title from localized app title', () => {
    render(<App />)

    expect(document.title).toBe(messages.en.app_title)
    expect(screen.getByLabelText(messages.en.language)).not.toBeNull()
    expect(screen.getByRole('columnheader', { name: messages.en.identifier })).not.toBeNull()
  })
})
