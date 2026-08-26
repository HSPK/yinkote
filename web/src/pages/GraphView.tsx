/** The relationship graph around one item.
 *
 *  Laid out with a small force simulation rather than a library: the graph is
 *  never more than a few dozen nodes, so the whole simulation is about thirty
 *  lines, and a layout engine would be the largest dependency in the app for a
 *  picture this size.
 *
 *  Drawn as SVG rather than canvas for the same reason — at this size the
 *  browser's own hit-testing, focus and text rendering are free, and a canvas
 *  would mean reimplementing all three.
 */
import { useEffect, useMemo, useRef, useState } from 'react'

import { api } from '../api/client'
import type { GraphEdge, GraphNode } from '../api/types'
import { useT } from '../i18n'
import { layout, type Placed, RELATIONS } from '../lib/graph'
import { useStore } from '../state/store'
import { Empty, Icon } from '../ui'

export function GraphView({ target }: { target?: string }) {
  const t = useT()
  const library = useStore((s) => s.library)
  const openReader = useStore((s) => s.openReader)
  const openGraph = useStore((s) => s.openGraph)
  const select = useStore((s) => s.select)
  const setGraphSize = useStore((s) => s.setGraphSize)

  const [nodes, setNodes] = useState<GraphNode[]>([])
  const [edges, setEdges] = useState<GraphEdge[]>([])
  const [error, setError] = useState<string | null>(null)
  const [hover, setHover] = useState<string | null>(null)
  const [size, setSize] = useState({ width: 720, height: 480 })
  const boxRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!target) return
    let live = true
    api.graph
      .around(library, target)
      .then((g) => {
        if (!live) return
        setNodes(g.nodes)
        setEdges(g.edges)
        setGraphSize(g.nodes.length, g.edges.length)
        setError(null)
      })
      .catch((e: unknown) => live && setError(e instanceof Error ? e.message : String(e)))
    return () => {
      live = false
    }
  }, [library, target, setGraphSize])

  // The layout depends on the pane's size, so it has to be measured rather
  // than assumed; a graph laid out for 720px in a 300px pane is a pile.
  useEffect(() => {
    const box = boxRef.current
    if (!box) return
    const observer = new ResizeObserver(() => {
      const rect = box.getBoundingClientRect()
      if (rect.width > 0 && rect.height > 0) setSize({ width: rect.width, height: rect.height })
    })
    observer.observe(box)
    return () => observer.disconnect()
  }, [])

  const placed = useMemo(
    () => layout(nodes, edges, size.width, size.height),
    [nodes, edges, size.width, size.height],
  )
  const at = useMemo(() => new Map(placed.map((p) => [p.key, p])), [placed])

  const open = (node: Placed) => {
    select(node.key)
    openReader(node.key)
  }

  if (!target) return <Empty>{t('graph.none')}</Empty>
  if (error) return <Empty>{error}</Empty>
  if (!nodes.length) return <Empty>{t('graph.empty')}</Empty>

  return (
    <div className="pane main graph" ref={boxRef}>
      <svg className="graph-canvas" width={size.width} height={size.height}>
        {edges.map((e, i) => {
          const a = at.get(e.source)
          const b = at.get(e.target)
          if (!a || !b) return null
          const lit = hover === e.source || hover === e.target
          return (
            <line
              key={i}
              className="graph-edge"
              data-relation={e.relation}
              data-lit={lit || undefined}
              x1={a.x}
              y1={a.y}
              x2={b.x}
              y2={b.y}
            />
          )
        })}

        {placed.map((node) => (
          <g
            key={node.key}
            className="graph-node"
            data-focus={node.focus || undefined}
            data-lit={hover === node.key || undefined}
            transform={`translate(${node.x} ${node.y})`}
            tabIndex={0}
            role="button"
            onMouseEnter={() => setHover(node.key)}
            onMouseLeave={() => setHover(null)}
            onClick={() => select(node.key)}
            onDoubleClick={() => (node.focus ? open(node) : openGraph(node.key))}
            onKeyDown={(e) => e.key === 'Enter' && open(node)}
          >
            <circle r={node.focus ? 9 : 6} />
            <text x={node.focus ? 14 : 11} y={4}>
              {node.title || node.key}
              {node.year ? ` · ${node.year}` : ''}
            </text>
          </g>
        ))}
      </svg>

      <div className="graph-legend">
        {RELATIONS.map((relation) => (
          <span key={relation} className="graph-key">
            <i className="graph-swatch" data-relation={relation} />
            {t(`graph.relation.${relation}`)}
          </span>
        ))}
        <span className="graph-hint">
          <Icon.Search size={12} />
          {t('graph.hint')}
        </span>
      </div>
    </div>
  )
}
