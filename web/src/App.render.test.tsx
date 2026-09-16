/**
 * Does the workbench actually render?
 *
 * Everything else is checked a layer down — types, pure functions, the API over
 * HTTP — and none of it would notice a module that evaluates to `undefined`,
 * a hook called from the wrong place, or a registry entry pointing at nothing.
 * Those show up only when React is asked to build the tree.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import { useStore } from './state/store'
import { TABS } from './workspace/registry'
import { libraryTab, tabId, type TabKind } from './lib/tabs'

// The workbench must not reach the network to draw itself.
// A client that never answers, shared so every page test agrees what that
// means. This file used to build its own a level too shallow — see
// `src/test/idleApi.ts`.
vi.mock('./api/client', async () => (await import('./test/idleApi')).idleClient())

let container: HTMLElement
let root: Root

async function render() {
  await act(async () => {
    root.render(<App />)
  })
}

beforeEach(() => {
  useStore.setState({ sidebarOpen: true })
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

describe('the workbench', () => {
  it('offers the top details toggle on every view that has a details panel', async () => {
    for (const kind of ['library', 'collections', 'chats', 'note', 'files', 'reader', 'graph'] as const) {
      useStore.setState({
        ready: true,
        tabs: [{ id: kind, kind, title: kind }],
        activeTab: kind,
        detailOpen: false,
        readerLayout: { ...useStore.getState().readerLayout, detailsOpen: false },
      })
      await render()
      const toggle = container.querySelector<HTMLButtonElement>('.toolbar [aria-controls="workspace-details"]')
      expect(toggle?.disabled, kind).toBe(false)
      await act(async () => toggle?.click())
      expect(container.querySelector('#workspace-details'), kind).not.toBeNull()
      await act(async () => toggle?.click())
      expect(container.querySelector('#workspace-details'), kind).toBeNull()
      expect(container.querySelector('.statusbar [title="Show details"]'), kind).toBeNull()
    }
  })

  it('can hide and reopen the left sidebar without losing its width', async () => {
    useStore.setState({ ready: true, tabs: [libraryTab('Library')], activeTab: 'library' })
    await render()
    const width = useStore.getState().layout.sidebar
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-controls="library-sidebar"]')?.click())
    expect(container.querySelector('#library-sidebar')).toBeNull()
    expect(container.querySelector('.workspace-main')).not.toBeNull()
    expect(useStore.getState().layout.sidebar).toBe(width)
    await act(async () => container.querySelector<HTMLButtonElement>('[aria-controls="library-sidebar"]')?.click())
    expect(container.querySelector('#library-sidebar')).not.toBeNull()
    expect(useStore.getState().layout.sidebar).toBe(width)
  })
  it('shows a connecting state before the server answers', async () => {
    useStore.setState({ ready: false })
    await render()
    expect(container.textContent).toContain('Yinkote')
  })

  it('draws the shell once ready', async () => {
    useStore.setState({ ready: true, tabs: [libraryTab('Library')] })
    await render()

    expect(container.querySelector('.toolbar'), 'the toolbar').toBeTruthy()
    expect(container.querySelector('.sidebar-nav'), 'the sidebar').toBeTruthy()
    expect(container.querySelector('.table-head'), 'the item table').toBeTruthy()
    expect(container.querySelector('.statusbar'), 'the status bar').toBeTruthy()
  })

  it('renders every registered tab kind', async () => {
    // A registry entry pointing at a component that throws on mount is
    // invisible to the type checker and to every other test here.
    for (const kind of Object.keys(TABS) as TabKind[]) {
      useStore.setState({
        ready: true,
        tabs: [{ id: tabId(kind), kind, title: kind }],
        activeTab: tabId(kind),
      })
      await render()
      expect(container.querySelector('.workspace-main'), kind).toBeTruthy()
    }
  })

  it('shows the detail pane only where the registry says it belongs', async () => {
    useStore.setState({
      ready: true,
      detailOpen: true,
      tabs: [{ id: tabId('plugins'), kind: 'plugins', title: '' }],
      activeTab: tabId('plugins'),
    })
    await render()
    expect(container.querySelector('.detail-pane'), 'plugins has no detail pane').toBeNull()

    useStore.setState({ tabs: [libraryTab('Library')], activeTab: 'library' })
    await render()
    expect(container.querySelector('.detail-pane'), 'the library does').toBeTruthy()
  })
})

describe('switching language', () => {
  it('redraws the chrome, rather than waiting for the next navigation', async () => {
    // The catalogues are checked for parity elsewhere; what is untested is that
    // a change reaches the tree at all — the store holding the locale is not
    // the store the components read their data from.
    const { useI18n } = await import('./i18n')

    useStore.setState({ ready: true, tabs: [libraryTab('Library')], activeTab: 'library' })
    act(() => useI18n.getState().setLocale('en-US'))
    await render()
    const english = container.querySelector('.nav-item')?.textContent ?? ''

    await act(async () => {
      useI18n.getState().setLocale('zh-CN')
    })
    const chinese = container.querySelector('.nav-item')?.textContent ?? ''

    expect(english, 'the English label').toBeTruthy()
    expect(chinese, 'redrawn without a remount').not.toBe(english)
  })
})
