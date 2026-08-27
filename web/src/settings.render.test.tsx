/**
 * Pointing the assistant at a model, rendered.
 *
 * The program is a local server the user started, so this has to work from the
 * workbench — telling somebody to edit a TOML file and restart would make the
 * web interface a partial one. And the key is write-only, which is the part
 * most easily got wrong in a way nobody notices until it is erased.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { AgentStatus } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const saved: Record<string, unknown>[] = []
let status: AgentStatus = { configured: false }

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        if (path === 'api.configureAgent') {
          const patch = args[0] as Record<string, unknown>
          saved.push(patch)
          status = {
            configured: true,
            endpoint: String(patch.endpoint ?? ''),
            model: String(patch.model ?? ''),
            hasApiKey: 'apiKey' in patch || !!status.hasApiKey,
            tools: ['search_library', 'get_item'],
          }
          return Promise.resolve(status)
        }
        if (path === 'api.scrape.sources') return Promise.resolve([])
        return new Promise(() => {})
      },
    })
  return { api: build('api'), connectEvents: () => () => {} }
})

beforeAll(() => {
  globalThis.ResizeObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never
  globalThis.IntersectionObserver ??= class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as never
  Element.prototype.scrollIntoView = () => {}
  Element.prototype.getBoundingClientRect = () =>
    ({ width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

let container: HTMLElement
let root: Root

beforeEach(() => {
  saved.length = 0
  status = { configured: false }
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: 'settings', kind: 'settings', title: 'Settings' }],
    activeTab: 'settings',
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
    agent: null,
  })
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

async function render() {
  await act(async () => {
    root.render(<App />)
  })
  await act(async () => {
    await Promise.resolve()
  })
}

const fields = () => [...container.querySelectorAll('.model-settings input')] as HTMLInputElement[]

async function fill(el: HTMLInputElement, value: string) {
  await act(async () => {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      'value',
    )!.set!
    setter.call(el, value)
    el.dispatchEvent(new Event('input', { bubbles: true }))
  })
}

const saveButton = () =>
  [...container.querySelectorAll('.model-settings button')].find((b) =>
    b.textContent?.includes('Save and connect'),
  ) as HTMLButtonElement

describe('configuring the model', () => {
  it('offers the fields in settings', async () => {
    await render()
    expect(fields()).toHaveLength(3)
  })

  it('will not save half a configuration', async () => {
    await render()
    await fill(fields()[0]!, 'http://127.0.0.1:11434/v1')
    // A model name is the other half; saving without it would fail at the
    // server, which is a worse place to find out.
    expect(saveButton().disabled).toBe(true)
  })

  it('saves the endpoint and the model', async () => {
    await render()
    await fill(fields()[0]!, 'http://127.0.0.1:11434/v1')
    await fill(fields()[1]!, 'llama3')
    await act(async () => saveButton().click())

    expect(saved[0]).toMatchObject({
      endpoint: 'http://127.0.0.1:11434/v1',
      model: 'llama3',
    })
  })

  it('leaves a stored key alone when the box is empty', async () => {
    useStore.setState({ agent: { configured: true, hasApiKey: true, endpoint: 'e', model: 'm' } })
    await render()
    await act(async () => saveButton().click())

    // The box is empty because the key is never shown, not because the user
    // cleared it. Sending an empty string would erase it on every save.
    expect(saved[0]).not.toHaveProperty('apiKey')
  })

  it('sends a key that was typed, and does not keep it in the box', async () => {
    await render()
    await fill(fields()[0]!, 'http://x/v1')
    await fill(fields()[1]!, 'm')
    await fill(fields()[2]!, 'secret')
    await act(async () => saveButton().click())

    expect(saved[0]).toMatchObject({ apiKey: 'secret' })
    expect(fields()[2]!.value).toBe('')
  })

  it('says whether the assistant is ready', async () => {
    await render()
    expect(container.querySelector('.model-settings')?.textContent).toContain('Not configured')

    await fill(fields()[0]!, 'http://x/v1')
    await fill(fields()[1]!, 'm')
    await act(async () => saveButton().click())

    expect(container.querySelector('.model-settings')?.textContent).toContain('Ready')
  })
})
