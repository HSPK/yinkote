import { describe, expect, it } from 'vitest'

import type { GraphEdge, GraphNode } from '../api/types'
import { layout } from './graph'

const node = (key: string, focus = false): GraphNode => ({
  key,
  title: key,
  itemType: 'journalArticle',
  ...(focus ? { focus: true } : {}),
})

const edge = (source: string, target: string, weight = 1): GraphEdge => ({
  source,
  target,
  relation: 'tag',
  weight,
})

const BOX = { width: 800, height: 600 }

describe('graph layout', () => {
  it('places nothing when there is nothing', () => {
    expect(layout([], [], BOX.width, BOX.height)).toEqual([])
  })

  it('is deterministic, so a graph looks the same each time it is opened', () => {
    const nodes = [node('F', true), node('A'), node('B')]
    const edges = [edge('F', 'A'), edge('F', 'B')]

    const once = layout(nodes, edges, BOX.width, BOX.height)
    const twice = layout(nodes, edges, BOX.width, BOX.height)
    expect(once.map((p) => [p.x, p.y])).toEqual(twice.map((p) => [p.x, p.y]))
  })

  it('keeps every node inside the box, labels included', () => {
    const nodes = [node('F', true), ...Array.from({ length: 20 }, (_, i) => node(`N${i}`))]
    const edges = nodes.slice(1).map((n) => edge('F', n.key))

    for (const p of layout(nodes, edges, BOX.width, BOX.height)) {
      // A node laid out off-screen is one the reader never learns exists.
      expect(p.x).toBeGreaterThanOrEqual(0)
      expect(p.y).toBeGreaterThanOrEqual(0)
      expect(p.x).toBeLessThanOrEqual(BOX.width)
      expect(p.y).toBeLessThanOrEqual(BOX.height)
    }
  })

  it('never produces a coordinate that is not a number', () => {
    // Two nodes at the same point have no direction to repel in, which is the
    // classic way a force layout produces NaN and renders nothing at all.
    const nodes = [node('F', true), node('A'), node('B')]
    const placed = layout(nodes, [edge('A', 'B')], BOX.width, BOX.height)
    for (const p of placed) {
      expect(Number.isFinite(p.x)).toBe(true)
      expect(Number.isFinite(p.y)).toBe(true)
    }
  })

  it('pulls a strongly related neighbour closer than a weak one', () => {
    const nodes = [node('F', true), node('Strong'), node('Weak')]
    const edges = [edge('F', 'Strong', 3), edge('F', 'Weak', 0.4)]

    const placed = layout(nodes, edges, BOX.width, BOX.height)
    const at = (key: string) => placed.find((p) => p.key === key)!
    const distance = (key: string) => Math.hypot(at(key).x - at('F').x, at(key).y - at('F').y)

    expect(distance('Strong')).toBeLessThan(distance('Weak'))
  })

  it('separates nodes that are not connected to each other', () => {
    const nodes = [node('F', true), node('A'), node('B')]
    const placed = layout(nodes, [edge('F', 'A'), edge('F', 'B')], BOX.width, BOX.height)
    const a = placed.find((p) => p.key === 'A')!
    const b = placed.find((p) => p.key === 'B')!

    expect(Math.hypot(a.x - b.x, a.y - b.y)).toBeGreaterThan(20)
  })

  it('ignores an edge pointing at a node that is not here', () => {
    const placed = layout([node('F', true)], [edge('F', 'ghost')], BOX.width, BOX.height)
    expect(placed).toHaveLength(1)
    expect(Number.isFinite(placed[0]!.x)).toBe(true)
  })

  it('lays out a graph with no focus node without crashing', () => {
    // The server always marks one, but a graph is not the place to find out
    // that it did not.
    const placed = layout([node('A'), node('B')], [edge('A', 'B')], BOX.width, BOX.height)
    expect(placed).toHaveLength(2)
  })
})
