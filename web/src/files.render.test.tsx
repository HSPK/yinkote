/**
 * The file browser and the download queue, rendered.
 *
 * Written before anybody has used them, because every surface added in the last
 * few rounds had a dead interaction or a crash that only rendering it found:
 * the graph node that selected nothing, the chat pane that blanked on a missing
 * array. Unit tests of the same code passed throughout.
 */
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { App } from './App'
import type { Download, LibraryFile } from './api/types'
import { emptyScope } from './state/scope'
import { useStore } from './state/store'

const calls: string[] = []

let files: LibraryFile[] = []
let downloads: Download[] = []
let plan = {
  template: '{author} {year} - {title}',
  total: 0,
  changes: [] as { key: string; from: string; to: string }[],
}

vi.mock('./api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: (_t, _this, args: unknown[]) => {
        calls.push(`${path}(${JSON.stringify(args.slice(1))})`)
        if (path === 'api.files.list') return Promise.resolve({ files, total: files.length })
        if (path === 'api.files.preview') return Promise.resolve(plan)
        if (path === 'api.files.rename') return Promise.resolve({ renamed: 2, failed: 0 })
        if (path === 'api.downloads.list')
          return Promise.resolve({
            downloads,
            waiting: downloads.filter((d) => d.state === 'waiting').length,
            failed: downloads.filter((d) => d.state === 'failed').length,
          })
        if (path === 'api.downloads.retry') return Promise.resolve({ retrying: 1 })
        if (path === 'api.downloads.remove') return Promise.resolve({ removed: 1 })
        if (path === 'api.downloads.clear') return Promise.resolve({ cleared: 1 })
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
  Element.prototype.getBoundingClientRect = () =>
    ({ width: 1200, height: 800, top: 0, left: 0, right: 1200, bottom: 800, x: 0, y: 0, toJSON: () => ({}) }) as DOMRect
  Object.defineProperty(HTMLElement.prototype, 'offsetHeight', { configurable: true, value: 800 })
})

const file = (over: Partial<LibraryFile> = {}): LibraryFile => ({
  key: 'FILE1111',
  parentKey: 'PAPER111',
  parentTitle: 'Attention is all you need',
  filename: '1-s2.0-S009286742030121X-main.pdf',
  contentType: 'application/pdf',
  url: 'https://example.org/paper.pdf',
  bytes: 2_215_244,
  ...over,
})

const download = (over: Partial<Download> = {}): Download => ({
  id: 1,
  itemKey: 'PAPER111',
  url: 'https://example.org/paper.pdf',
  state: 'waiting',
  attempts: 0,
  error: '',
  title: 'A paper',
  bytes: 0,
  updatedAt: 0,
  ...over,
})

let container: HTMLElement
let root: Root

function mount(tab: 'files' | 'downloads') {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
  useStore.setState({
    ...emptyScope({ items: [], total: 0 }),
    ready: true,
    tabs: [{ id: tab, kind: tab, title: tab }],
    activeTab: tab,
    scopes: {},
    collections: [],
    smartCollections: [],
    tags: [],
    badgeDefs: [],
  })
}

async function render() {
  await act(async () => {
    root.render(<App />)
  })
  await act(async () => {
    await Promise.resolve()
  })
}

beforeEach(() => {
  calls.length = 0
  files = [file()]
  downloads = [download()]
  plan = { template: '{author} {year} - {title}', total: 0, changes: [] }
})

afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

describe('the file browser', () => {
  beforeEach(() => mount('files'))

  it('shows the name on disk beside the paper it belongs to', async () => {
    await render()

    const row = container.querySelector('.files-grid.row')
    expect(row?.textContent).toContain('1-s2.0-S009286742030121X-main.pdf')
    expect(row?.textContent).toContain('Attention is all you need')
  })

  it('says where the file came from', async () => {
    await render()

    // The question a file browser is opened to answer.
    expect(container.querySelector('.files-grid.row')?.textContent).toContain('example.org')
  })

  it('reads the size from disk in units a person can read', async () => {
    await render()
    expect(container.textContent).toContain('2.1 MB')
  })

  it('previews a rename without renaming anything', async () => {
    plan = {
      template: '{author} {year} - {title}',
      total: 1,
      changes: [{ key: 'FILE1111', from: 'old.pdf', to: 'Vaswani 2017 - Attention.pdf' }],
    }
    await render()

    const preview = [...container.querySelectorAll('button')].find(
      (b) => b.textContent === 'Preview rename',
    )!
    await act(async () => preview.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    await act(async () => {
      await Promise.resolve()
    })

    // Shown, and nothing asked to change: a batch rename nobody can look at
    // first is one nobody should run.
    expect(container.textContent).toContain('Vaswani 2017 - Attention.pdf')
    expect(calls.some((c) => c.startsWith('api.files.rename'))).toBe(false)
  })

  it('reports the whole count even though it only shows a sample', async () => {
    plan = {
      template: '{author} {year} - {title}',
      total: 30_000,
      changes: [{ key: 'FILE1111', from: 'old.pdf', to: 'new.pdf' }],
    }
    await render()
    const preview = [...container.querySelectorAll('button')].find(
      (b) => b.textContent === 'Preview rename',
    )!
    await act(async () => preview.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    await act(async () => {
      await Promise.resolve()
    })

    // Sending every row was 3.7 MB for a panel that shows eight lines. The
    // number is the answer; the rows are only the evidence.
    expect(container.textContent).toContain('30000')
  })

  it('will not rename until a preview says there is something to do', async () => {
    await render()

    const rename = [...container.querySelectorAll('button')].find(
      (b) => b.textContent === 'Rename all',
    ) as HTMLButtonElement
    expect(rename.disabled).toBe(true)
  })
})

describe('the download queue', () => {
  beforeEach(() => mount('downloads'))

  it('shows what state each file is in', async () => {
    downloads = [download({ state: 'running' })]
    await render()
    expect(container.querySelector('.download-state')?.textContent).toBe('Downloading')
  })

  it('keeps the reason a download failed beside it', async () => {
    downloads = [download({ state: 'failed', error: '403 Forbidden' })]
    await render()

    // A log is where nobody looks, and the reason is what a retry is decided
    // from.
    expect(container.querySelector('.download-error')?.textContent).toBe('403 Forbidden')
  })

  it('offers a retry only for what failed', async () => {
    downloads = [download({ id: 1, state: 'waiting' }), download({ id: 2, state: 'failed' })]
    await render()

    const retries = [...container.querySelectorAll('.row button')].filter(
      (b) => b.textContent === 'Retry',
    )
    expect(retries).toHaveLength(1)
  })

  it('actually retries when asked', async () => {
    downloads = [download({ id: 7, state: 'failed', error: 'timed out' })]
    await render()

    const retry = [...container.querySelectorAll('.row button')].find(
      (b) => b.textContent === 'Retry',
    )!
    await act(async () => retry.dispatchEvent(new MouseEvent('click', { bubbles: true })))
    await act(async () => {
      await Promise.resolve()
    })

    expect(calls.some((c) => c.startsWith('api.downloads.retry([[7]])'))).toBe(true)
  })

  it('counts what needs attention for the sidebar', async () => {
    downloads = [download({ id: 1, state: 'waiting' }), download({ id: 2, state: 'failed' })]
    await render()

    // A badge that counts finished downloads is a badge nobody can clear.
    expect(useStore.getState().downloadCount).toBe(2)
  })
})
