/** Laying out a small graph.
 *
 *  A force simulation, run to completion synchronously rather than animated.
 *  An animation would be the only moving thing in an otherwise still
 *  workbench, and it makes a reader wait to find out what they are looking at.
 *  At this size — a few dozen nodes — the whole thing settles in under a
 *  millisecond, so there is nothing to watch anyway.
 *
 *  Deterministic on purpose: the same neighbourhood must lay out the same way
 *  every time it is opened, or a reader cannot recognise a graph they have
 *  seen before. That rules out random starting positions.
 */
import type { GraphEdge, GraphNode } from '../api/types'

export const RELATIONS = ['tag', 'author', 'collection', 'coupling', 'similar', 'cites'] as const

export type Relation = (typeof RELATIONS)[number]

export interface Placed extends GraphNode {
  x: number
  y: number
}

const ITERATIONS = 220
/** How hard unconnected nodes push apart. */
const REPULSION = 9000
/** How hard an edge pulls, per unit of length beyond its rest length. */
const SPRING = 0.02
const REST_LENGTH = 110
/** Velocity retained per step; without damping the simulation never settles. */
const DAMPING = 0.82

/**
 * Place nodes in a box.
 *
 * The focus starts at the centre and is pinned there: it is the thing the graph
 * is *about*, and letting it drift means the reader has to find it again after
 * every layout.
 */
export function layout(
  nodes: GraphNode[],
  edges: GraphEdge[],
  width: number,
  height: number,
): Placed[] {
  if (!nodes.length) return []

  const cx = width / 2
  const cy = height / 2
  const focusKey = nodes.find((n) => n.focus)?.key

  // Start on a circle rather than at random, so the layout is reproducible and
  // no two nodes begin on top of each other (where repulsion has no direction).
  const others = nodes.filter((n) => n.key !== focusKey)
  const radius = Math.min(width, height) * 0.34
  const placed: Placed[] = nodes.map((node) => {
    if (node.key === focusKey) return { ...node, x: cx, y: cy }
    const i = others.indexOf(node)
    const angle = (i / Math.max(others.length, 1)) * Math.PI * 2
    return { ...node, x: cx + Math.cos(angle) * radius, y: cy + Math.sin(angle) * radius }
  })

  const index = new Map(placed.map((p, i) => [p.key, i]))
  const links = edges
    .map((e) => ({ a: index.get(e.source), b: index.get(e.target), weight: e.weight }))
    .filter((l): l is { a: number; b: number; weight: number } => l.a != null && l.b != null)

  const vx = new Array<number>(placed.length).fill(0)
  const vy = new Array<number>(placed.length).fill(0)

  for (let step = 0; step < ITERATIONS; step += 1) {
    for (let i = 0; i < placed.length; i += 1) {
      for (let j = i + 1; j < placed.length; j += 1) {
        const a = placed[i]!
        const b = placed[j]!
        let dx = a.x - b.x
        let dy = a.y - b.y
        let d2 = dx * dx + dy * dy
        if (d2 < 1) {
          // Exactly coincident nodes have no direction to push in, so give
          // them one rather than dividing by zero and producing NaN.
          dx = (i % 2 ? 1 : -1) * 0.5
          dy = 0.5
          d2 = 0.5
        }
        const force = REPULSION / d2
        const d = Math.sqrt(d2)
        vx[i]! += (dx / d) * force
        vy[i]! += (dy / d) * force
        vx[j]! -= (dx / d) * force
        vy[j]! -= (dy / d) * force
      }
    }

    for (const link of links) {
      const a = placed[link.a]!
      const b = placed[link.b]!
      const dx = b.x - a.x
      const dy = b.y - a.y
      const d = Math.hypot(dx, dy) || 1
      // A stronger relationship pulls harder, so the picture reads as strength
      // without needing a legend for line thickness.
      const pull = SPRING * (d - REST_LENGTH) * Math.min(Math.max(link.weight, 0.4), 3)
      vx[link.a]! += (dx / d) * pull
      vy[link.a]! += (dy / d) * pull
      vx[link.b]! -= (dx / d) * pull
      vy[link.b]! -= (dy / d) * pull
    }

    for (let i = 0; i < placed.length; i += 1) {
      const node = placed[i]!
      if (node.key === focusKey) {
        vx[i] = 0
        vy[i] = 0
        continue
      }
      vx[i]! *= DAMPING
      vy[i]! *= DAMPING
      node.x += vx[i]!
      node.y += vy[i]!
    }
  }

  return clampIntoView(placed, width, height)
}

/** Labels extend to the right, so the margin is not symmetric. */
const MARGIN = { left: 24, right: 190, top: 20, bottom: 20 }

/**
 * Scale and shift the result so every node is visible.
 *
 * A node laid out off-screen is a node the reader will never know was there,
 * which is worse than a slightly cramped picture.
 */
function clampIntoView(placed: Placed[], width: number, height: number): Placed[] {
  const xs = placed.map((p) => p.x)
  const ys = placed.map((p) => p.y)
  const minX = Math.min(...xs)
  const maxX = Math.max(...xs)
  const minY = Math.min(...ys)
  const maxY = Math.max(...ys)

  const usableW = Math.max(width - MARGIN.left - MARGIN.right, 80)
  const usableH = Math.max(height - MARGIN.top - MARGIN.bottom, 80)
  const scale = Math.min(usableW / Math.max(maxX - minX, 1), usableH / Math.max(maxY - minY, 1), 1)

  return placed.map((p) => ({
    ...p,
    x: MARGIN.left + (p.x - minX) * scale,
    y: MARGIN.top + (p.y - minY) * scale,
  }))
}
