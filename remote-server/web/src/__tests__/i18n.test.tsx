import { cleanup, render, screen } from '@testing-library/react'
import { afterEach, describe, expect, it } from 'vitest'
import { App } from '../App.js'
import { detectLanguage } from '../i18n/index.js'
import { messages, supportedLanguages } from '../i18n/messages.js'

afterEach(() => {
  cleanup()
})

describe('i18n messages', () => {
  it('includes console chrome labels for every supported language', () => {
    const englishKeys = Object.keys(messages.en).sort()

    for (const language of supportedLanguages) {
      expect(Object.keys(messages[language]).sort()).toEqual(englishKeys)
      expect(messages[language].language).toBeTruthy()
      expect(messages[language].identifier).toBeTruthy()
    }
  })

  it('falls back safely when browser language is unavailable or unsupported', () => {
    const originalNavigator = globalThis.navigator

    Reflect.deleteProperty(globalThis, 'navigator')
    expect(detectLanguage()).toBe('en')

    Object.defineProperty(globalThis, 'navigator', {
      configurable: true,
      value: originalNavigator
    })
    expect(detectLanguage('fr-FR')).toBe('en')
  })

  it('maps Chinese regions to simplified or traditional variants', () => {
    expect(detectLanguage('zh-TW')).toBe('zh-TW')
    expect(detectLanguage('zh-HK')).toBe('zh-TW')
    expect(detectLanguage('zh-MO')).toBe('zh-TW')
    expect(detectLanguage('zh-Hant')).toBe('zh-TW')
    expect(detectLanguage('zh-Hant-HK')).toBe('zh-TW')
    expect(detectLanguage('zh-Hant-MO')).toBe('zh-TW')
    expect(detectLanguage('zh-Hant-TW')).toBe('zh-TW')
    expect(detectLanguage('zh-CN')).toBe('zh-CN')
    expect(detectLanguage('zh-SG')).toBe('zh-CN')
    expect(detectLanguage('zh-Hans')).toBe('zh-CN')
    expect(detectLanguage('zh-Hans-CN')).toBe('zh-CN')
    expect(detectLanguage('zh-Hans-SG')).toBe('zh-CN')
    expect(detectLanguage('zh-Hans-HK')).toBe('zh-CN')
    expect(detectLanguage('zh')).toBe('zh-CN')
  })

  it('sets the document title from localized app title', () => {
    render(<App />)

    expect(document.title).toBe(messages.en.app_title)
    expect(document.documentElement.lang).toBe('en')
    expect(screen.getByLabelText(messages.en.language)).not.toBeNull()
    expect(screen.getByRole('textbox', { name: messages.en.email })).not.toBeNull()
  })
})
