import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'
import { App } from '../App.js'

describe('App smoke', () => {
  it('renders the remote client login entry', () => {
    render(<App />)

    expect(screen.getByRole('heading', { name: 'NiuMa Remote Client' })).toBeInTheDocument()
  })
})
