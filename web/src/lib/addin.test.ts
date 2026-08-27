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
      'bibliographyHtml',
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
