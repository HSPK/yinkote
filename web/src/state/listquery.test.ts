/**
 * What the item list actually asks the server for.
 *
 * The table is flat, so browsing filters to top-level items: a highlight has
 * no way to be shown under its paper and appears as a blank row between two
 * papers instead. But that filter is wrong in two other places, and both are
 * easy to miss because the browse case is the one anybody tests:
 *
 *  - **Searching** must reach children. The phrase a reader highlighted lives
 *    on the annotation, not on the paper, so filtering answers "no results"
 *    for text the user knows they marked.
 *  - **The trash** must too. Trashing an attachment on its own leaves a paper
 *    that is not deleted, so a top-level trash showed nothing: the file could
 *    neither be restored nor emptied. That regression was introduced by the
 *    change that added the filter and caught by asking what else browses.
 */
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { emptyScope } from './scope'
import { useStore } from './store'

vi.mock('../api/client', () => {
  const build = (path: string): unknown =>
    new Proxy(function () {} as object, {
      get: (_t, key) => (key === 'then' ? undefined : build(`${path}.${String(key)}`)),
      apply: () => new Promise(() => {}),
    })
  return { api: build('api'), connectEvents: () => () => {} }
})

const query = () => useStore.getState().listQuery()

beforeEach(() => {
  useStore.setState({ ...emptyScope(), scopes: {} })
})

describe('listQuery decides who is included', () => {
  it('asks for papers only when browsing the library', () => {
    useStore.setState({ view: 'library', query: '' })
    expect(query().topLevel).toBe(true)
  })

  it('asks for everything when searching', () => {
    useStore.setState({ view: 'library', query: 'erosion' })
    expect(query().topLevel).toBeUndefined()
  })

  it('asks for everything in the trash, parent or not', () => {
    // A trashed attachment whose paper is untouched is top-level to nobody.
    useStore.setState({ view: 'trash', query: '' })
    expect(query().topLevel, 'a trashed file would be unreachable').toBeUndefined()
    expect(query().trash).toBe('only')
  })

  it('asks for papers only inside a collection', () => {
    // Children are never filed in collections, so this is the browse case
    // again — but it is a different branch and worth naming.
    useStore.setState({ view: 'collection', collection: 'ABCD1234', query: '' })
    expect(query().topLevel).toBe(true)
  })

  it('treats a box holding only whitespace as no search at all', () => {
    // `'   '` is truthy, so a single space counted as a search: the list
    // switched from papers to everything, attachments and highlights appeared
    // among the rows, and the total moved by 137 while the sidebar count --
    // fixed one round earlier to agree with it -- stayed where it was.
    useStore.setState({ view: 'library', query: '   ' })
    expect(query().q, 'whitespace was sent as a query').toBeUndefined()
    expect(query().topLevel, 'a stray space changed which rows are listed').toBe(true)
    expect(query().mode).toBeUndefined()
  })

  it('keeps the spaces inside a real phrase', () => {
    // Trimming the ends must not touch the middle: multi-word searching is
    // the ordinary case, and it has been broken here before.
    useStore.setState({ view: 'library', query: '  attention is all you need  ' })
    expect(query().q).toBe('attention is all you need')
    expect(query().topLevel).toBeUndefined()
  })
})
