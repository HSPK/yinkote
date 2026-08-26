/** Collection-tree shaping.
 *
 *  Extracted from the sidebar component so the ordering and nesting rules can
 *  be tested without rendering anything.
 */
import type { Collection } from '../api/types'

export interface TreeNode extends Collection {
  children: TreeNode[]
  depth: number
}

/**
 * Flatten collections into render order while preserving hierarchy.
 *
 * A collection whose parent is missing (deleted, or filtered out) is promoted
 * to a root rather than silently dropped — losing a user's folder from the
 * sidebar is far worse than showing it in the wrong place.
 */
export function buildTree(collections: Collection[]): TreeNode[] {
  const nodes = new Map<string, TreeNode>()
  for (const c of collections) nodes.set(c.key, { ...c, children: [], depth: 0 })

  const roots: TreeNode[] = []
  for (const node of nodes.values()) {
    const parent = node.parentKey ? nodes.get(node.parentKey) : undefined
    if (parent && parent !== node) parent.children.push(node)
    else roots.push(node)
  }

  const out: TreeNode[] = []
  const seen = new Set<string>()
  const walk = (list: TreeNode[], depth: number) => {
    list.sort((a, b) => a.sortIndex - b.sortIndex || a.name.localeCompare(b.name))
    for (const n of list) {
      // A cycle in the data must not hang the UI.
      if (seen.has(n.key)) continue
      seen.add(n.key)
      n.depth = depth
      out.push(n)
      walk(n.children, depth + 1)
    }
  }
  walk(roots, 0)
  return out
}
