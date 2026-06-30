import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { LoginPage } from '../auth/loginPage.js'
import { I18nProvider } from '../i18n/index.js'

function renderLoginPage(onLogin = vi.fn()) {
  render(
    <I18nProvider>
      <LoginPage onLogin={onLogin} />
    </I18nProvider>
  )
  return onLogin
}

describe('LoginPage', () => {
  it('submits email and password to the login callback', async () => {
    const onLogin = renderLoginPage()

    fireEvent.change(screen.getByLabelText('Email'), {
      target: { value: 'user@example.com' }
    })
    fireEvent.change(screen.getByLabelText('Password'), {
      target: { value: 'secret-password' }
    })
    fireEvent.click(screen.getByRole('button', { name: /sign in/i }))

    await waitFor(() => {
      expect(onLogin).toHaveBeenCalledWith('user@example.com', 'secret-password')
    })
  })
})
