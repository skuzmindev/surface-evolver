import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import ErrorBoundary from './ErrorBoundary'

// Throws until `broken.now` is cleared, so a retry can succeed.
const broken = { now: true }
function Flaky() {
  if (broken.now) throw new Error('no WebGL context')
  return <p>viewer content</p>
}

describe('ErrorBoundary', () => {
  it('replaces only the named pane and re-mounts it on Try again (#33)', async () => {
    vi.spyOn(console, 'error').mockImplementation(() => {})
    broken.now = true
    render(
      <div>
        <ErrorBoundary name="viewer"><Flaky /></ErrorBoundary>
        <p>command line still here</p>
      </div>,
    )

    expect(screen.getByRole('alert')).toHaveTextContent('The viewer stopped working: no WebGL context')
    expect(screen.getByText('command line still here')).toBeInTheDocument()

    broken.now = false
    await userEvent.click(screen.getByRole('button', { name: 'Try again' }))
    expect(screen.getByText('viewer content')).toBeInTheDocument()
    expect(screen.queryByRole('alert')).not.toBeInTheDocument()
  })
})
