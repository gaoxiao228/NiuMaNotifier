import { describe, expect, it } from 'vitest'

import { HttpError } from '../api/httpClient.js'
import { createTranslator } from '../i18n/index.js'
import { toDisplayErrorMessage } from '../shared/errorMessage.js'

describe('toDisplayErrorMessage', () => {
  it('translates client-side error keys in the UI layer', () => {
    const t = createTranslator('en')

    expect(toDisplayErrorMessage(t, new HttpError(0, 'api_error_network'), 'error')).toBe(
      'Network request failed'
    )
  })
})
