/**
 * The Word task pane's pure helpers.
 *
 * The pane is served straight from the Rust binary and loaded by Word with no
 * build step, so it is plain script rather than a module in this project's
 * graph. To test it we load the same file the server embeds and run it in a
 * `vm` context — which also means a syntax error in the pane fails the build
 * here, rather than silently in an author's copy of Word.
 *
 * Deliberately *not* a `.dom.test.ts`: the sandbox supplies its own globals,
 * and under jsdom `import.meta.url` is an http URL that `readFileSync` cannot
 * open.
 */

import { readFileSync } from 'node:fs'
import { createContext, runInContext } from 'node:vm'
import { describe, expect, it } from 'vitest'

const SOURCE = new URL(
  '../../../crates/yk-server/src/addin/assets/taskpane.js',
  import.meta.url,
)

/** The helpers the pane exports for testing. */
interface Pane {
  describeItem(item: Record<string, unknown>): string
  bibliographyHtml(plan: { bibliography?: { text: string }[] }): string
  describe(error: unknown): string
}

function load(): Pane {
  const sandbox: Record<string, unknown> = {
    module: { exports: {} },
    // The pane guards its own bootstrap on these; leaving them undefined is
    // exactly the state we want, because it means the file is inert on load.
    window: undefined,
    document: undefined,
    Office: undefined,
    fetch: undefined,
    setTimeout,
    clearTimeout,
  }
  runInContext(readFileSync(SOURCE, 'utf8'), createContext(sandbox))
  return (sandbox.module as { exports: Pane }).exports
}

describe('the Word task pane', () => {
  const pane = load()

  it('loads without touching Word or the network', () => {
    // If the file ever runs something at import time, this suite is where it
    // will fail: the sandbox has no document, no Office and no fetch.
    expect(Object.keys(pane as object).sort()).toEqual([
      'askForKey',
      'bibliographyHtml',
      'boot',
      'describe',
      'describeItem',
    ])
  })

  it('describes an item the way a citation picker should', () => {
    expect(
      pane.describeItem({
        creators: [{ lastName: 'Vaswani' }, { lastName: 'Shazeer' }],
        date: '2017-06-12',
        publicationTitle: 'NeurIPS',
      }),
    ).toBe('Vaswani & Shazeer · 2017 · NeurIPS')
  })

  it('collapses a long author list rather than filling the pane', () => {
    const many = Array.from({ length: 9 }, (_, i) => ({ lastName: `Author${i}` }))
    expect(pane.describeItem({ creators: many, date: '2020' })).toBe('Author0 et al. · 2020')
  })

  it('leaves out what an item does not have', () => {
    // No separators dangling: an item with only a title is common in a library
    // that was just imported.
    expect(pane.describeItem({})).toBe('')
    expect(pane.describeItem({ date: '2019-01-01' })).toBe('2019')
  })

  it('handles creators recorded as a single name', () => {
    expect(pane.describeItem({ creators: [{ name: 'World Health Organization' }] })).toBe(
      'World Health Organization',
    )
  })

  it('renders a bibliography as one paragraph per entry', () => {
    const html = pane.bibliographyHtml({
      bibliography: [{ text: 'Lovelace, A. (1843).' }, { text: 'Babbage, C. (1837).' }],
    })
    expect(html).toBe('<p>Lovelace, A. (1843).</p><p>Babbage, C. (1837).</p>')
  })

  it('renders an empty bibliography as an empty paragraph, not nothing', () => {
    // Word's insertHtml with an empty string leaves the previous content in
    // place, so removing the last citation has to write something.
    expect(pane.bibliographyHtml({ bibliography: [] })).toBe('<p></p>')
    expect(pane.bibliographyHtml({})).toBe('<p></p>')
  })

  it('shortens an error to something that fits a 300px pane', () => {
    const wordy = new Error(`RichApi.Error: something failed\n    at foo\n    at bar`)
    expect(pane.describe(wordy)).toBe('RichApi.Error: something failed')
    expect(pane.describe({ status: 401 })).toBe('The server wants an API key.')
    expect(pane.describe(null)).toBe('Something went wrong.')
    expect(pane.describe(new Error('x'.repeat(400))).length).toBe(160)
  })
})

/**
 * The key gate.
 *
 * A protected library serves the pane's assets without a key -- it must, or
 * Word could not load the pane at all -- and then answers 401 to everything
 * else. The pane read a key from localStorage and nothing ever wrote one,
 * with no field to type it into: it reported "The server wants an API key"
 * and stopped. That is the dead end the workbench had before it grew a gate,
 * left standing in the other client.
 */
describe('the task pane when the library wants a key', () => {
  /** A DOM small enough to boot the pane against, and honest about hidden. */
  function fakeDom(stored: string | null) {
    const nodes: Record<string, Record<string, unknown>> = {}
    for (const id of ['boot', 'app', 'keygate', 'keygate-input', 'keygate-error']) {
      nodes[id] = {
        hidden: id !== 'boot',
        textContent: '',
        value: '',
        focus: () => {},
        addEventListener: () => {},
      }
    }
    for (const id of ['keygate-save']) {
      nodes[id] = { addEventListener: () => {}, hidden: false }
    }
    const store: Record<string, string> = {}
    if (stored !== null) store['yinkote.apiKey'] = stored
    return { nodes, store }
  }

  function loadWithDom(stored: string | null, status: number) {
    const { nodes, store } = fakeDom(stored)
    const sandbox: Record<string, unknown> = {
      module: { exports: {} },
      document: { getElementById: (id: string) => nodes[id] ?? null },
      window: { localStorage: { getItem: (k: string) => store[k] ?? null, setItem: () => {} } },
      Office: undefined,
      fetch: async () => ({ ok: false, status, json: async () => ({ title: 'unauthorised' }) }),
      setTimeout,
      clearTimeout,
    }
    runInContext(readFileSync(SOURCE, 'utf8'), createContext(sandbox))
    return { pane: (sandbox.module as { exports: Pane & { boot(): Promise<void> } }).exports, nodes }
  }

  it('asks for a key instead of reporting that one is needed', async () => {
    const { pane, nodes } = loadWithDom(null, 401)
    await pane.boot()

    expect(nodes.keygate!.hidden, 'the pane stopped at a message').toBe(false)
    expect(nodes.boot!.hidden).toBe(true)
    // Nothing was tried, so there is nothing to call a failure yet.
    expect(nodes['keygate-error']!.hidden).toBe(true)
  })

  it('says so when the stored key is the one being refused', async () => {
    // Otherwise the pane asks again with the box apparently already right,
    // and the user has no way to tell a wrong key from a broken server.
    const { pane, nodes } = loadWithDom('stale-key', 401)
    await pane.boot()

    expect(nodes.keygate!.hidden).toBe(false)
    expect(nodes['keygate-error']!.hidden).toBe(false)
    expect(String(nodes['keygate-error']!.textContent)).toContain('not accepted')
  })

  it('still reports other failures as failures', async () => {
    // A gate shown for a server that is simply down would send the user
    // hunting for a key they do not need.
    const { pane, nodes } = loadWithDom(null, 500)
    await pane.boot()

    expect(nodes.keygate!.hidden).toBe(true)
    expect(String(nodes.boot!.textContent)).toContain('Cannot reach Yinkote')
  })
})
