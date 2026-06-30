import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { App } from '../App.js'

describe('App smoke', () => {
  it('renders the remote client login form', () => {
    render(<App />)

    expect(screen.getByRole('heading', { name: 'Sign in to remote client' })).toBeInTheDocument()
    expect(screen.getByLabelText('Email')).toBeInTheDocument()
  })
})
