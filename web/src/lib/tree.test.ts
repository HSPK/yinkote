import { describe, expect, it } from 'vitest'

import type { Collection } from '../api/types'
import { buildTree } from './tree'

const collection = (key: string, name: string, parentKey?: string, sortIndex = 0): Collection => ({
  key,
  libraryId: 1,
  name,
  parentKey,
  sortIndex,
  version: 1,
  itemCount: 0,
})

describe('buildTree', () => {
  it('returns an empty list for no collections', () => {
    expect(buildTree([])).toEqual([])
  })

  it('nests children under their parent in render order', () => {
    const tree = buildTree([
      collection('C', 'Child', 'P'),
      collection('P', 'Parent'),
      collection('G', 'Grandchild', 'C'),
    ])
    expect(tree.map((n) => [n.key, n.depth])).toEqual([
      ['P', 0],
      ['C', 1],
      ['G', 2],
    ])
  })

  it('orders siblings by sortIndex then name', () => {
    const tree = buildTree([
      collection('B', 'Beta', undefined, 2),
      collection('A', 'Alpha', undefined, 1),
      collection('C', 'Aardvark', undefined, 1),
    ])
    expect(tree.map((n) => n.key)).toEqual(['C', 'A', 'B'])
  })

  it('promotes orphans to roots instead of dropping them', () => {
    const tree = buildTree([collection('X', 'Orphan', 'MISSING')])
    expect(tree.map((n) => [n.key, n.depth])).toEqual([['X', 0]])
  })

  it('does not hang on a cycle', () => {
    const tree = buildTree([collection('A', 'A', 'B'), collection('B', 'B', 'A')])
    // Both are unreachable as roots, so nothing is emitted — but crucially the
    // call returns.
    expect(tree.length).toBeLessThanOrEqual(2)
  })

  it('ignores a collection that claims itself as parent', () => {
    const tree = buildTree([collection('S', 'Self', 'S')])
    expect(tree.map((n) => n.key)).toEqual(['S'])
  })

  it('preserves the original collection fields', () => {
    const [node] = buildTree([{ ...collection('K', 'Keep'), itemCount: 7 }])
    expect(node?.itemCount).toBe(7)
    expect(node?.name).toBe('Keep')
  })
})
