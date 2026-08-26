import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { ErrorBoundary } from './ErrorBoundary'

function Boom({ throws }: { throws: boolean }) {
  if (throws) throw new Error('the page would not draw')
  return <p>fine</p>
}

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  // React logs the caught error itself; the test is not interested.
  vi.spyOn(console, 'error').mockImplementation(() => {})
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
  vi.restoreAllMocks()
})

const render = async (node: React.ReactNode) => {
  await act(async () => {
    root.render(node)
  })
}

describe('ErrorBoundary', () => {
  it('passes children through when nothing goes wrong', async () => {
    await render(
      <ErrorBoundary>
        <Boom throws={false} />
      </ErrorBoundary>,
    )
    expect(container.textContent).toContain('fine')
  })

  it('shows the failure instead of blanking the window', async () => {
    await render(
      <ErrorBoundary>
        <Boom throws />
      </ErrorBoundary>,
    )
    expect(container.textContent).toContain('the page would not draw')
    expect(container.querySelector('.surface-error'), 'something is still drawn').toBeTruthy()
  })

  it('tries again when the surface changes', async () => {
    // Switching tabs is a fresh attempt; a boundary that stayed broken would
    // make one bad item poison the pane for the rest of the session.
    await render(
      <ErrorBoundary resetKey="a">
        <Boom throws />
      </ErrorBoundary>,
    )
    expect(container.querySelector('.surface-error')).toBeTruthy()

    await render(
      <ErrorBoundary resetKey="b">
        <Boom throws={false} />
      </ErrorBoundary>,
    )
    expect(container.textContent).toContain('fine')
  })

  it('does not retry on its own while the surface is unchanged', async () => {
    // Re-rendering into the same failure would loop.
    await render(
      <ErrorBoundary resetKey="a">
        <Boom throws />
      </ErrorBoundary>,
    )
    await render(
      <ErrorBoundary resetKey="a">
        <Boom throws />
      </ErrorBoundary>,
    )
    expect(container.querySelector('.surface-error')).toBeTruthy()
  })
})
