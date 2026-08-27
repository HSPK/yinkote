/** The workspace's tabs.
 *
 *  A reference manager is not a dialog-driven application: reading a PDF,
 *  annotating it, asking about it and browsing the library are all things a
 *  user does *alongside* each other, sometimes for an hour. Modals cannot
 *  express that — they take the screen, stack badly and lose their state on
 *  close.
 *
 *  A tab is therefore data, and the registry maps a kind to a component. Adding
 *  a surface is one entry in each, which is also what will let plugins
 *  contribute their own.
 */

export type TabKind =
  | 'library'
  | 'collections'
  | 'chat'
  | 'reader'
  | 'graph'
  | 'gaps'
  | 'duplicates'
  | 'downloads'
  | 'files'
  | 'plugins'
  | 'status'
  | 'settings'
  | 'collection-edit'

export interface Tab {
  /** Stable across reopening, so asking for the same thing twice focuses it. */
  id: string
  kind: TabKind
  /** What the tab bar shows; resolved when opened, not stored per render. */
  title: string
  /** Kind-specific target: a conversation key, an item key, and so on. */
  target?: string
  /**
   * A preview tab, shown in italics and reused by the next preview.
   *
   * Clicking through a list of papers should not leave twenty tabs behind. So
   * a single glance-at reuses one slot until the user says otherwise — by
   * double-clicking the tab, or by editing in it. Borrowed from editors
   * because it is the solved version of this problem.
   */
  preview?: boolean
}

export const LIBRARY_TAB_ID = 'library'

/** The tab every session starts with.
 *
 *  It starts open, not pinned open. A tab that cannot be closed is a second
 *  kind of tab, and one exception is enough to make every rule about tabs
 *  read "…except the library". Closing the last one leaves an empty
 *  workspace, which is a state the workbench can say something useful in.
 */
export function libraryTab(title: string): Tab {
  return { id: LIBRARY_TAB_ID, kind: 'library', title }
}

/** One id per thing, so opening the same item twice focuses the first tab. */
export function tabId(kind: TabKind, target?: string): string {
  return target ? `${kind}:${target}` : kind
}

/**
 * Open a tab, or focus it when it is already open.
 *
 * Returns the same array when nothing changed, so React can skip the render —
 * clicking the active tab is common and should cost nothing.
 */
export function openTab(tabs: Tab[], tab: Tab): Tab[] {
  const existing = tabs.findIndex((t) => t.id === tab.id)

  if (existing >= 0) {
    const found = tabs[existing]!
    // Re-opening keeps a tab that was already kept, and may carry a fresher
    // title — a renamed conversation, say.
    const preview = found.preview && tab.preview
    if (found.title === tab.title && found.preview === preview) return tabs
    return tabs.map((t) => (t.id === tab.id ? { ...t, title: tab.title, preview } : t))
  }

  if (!tab.preview) return [...tabs, tab]

  // A new preview takes over the slot the last one held, keeping its position
  // so the bar does not shuffle under the pointer.
  const slot = tabs.findIndex((t) => t.preview)
  if (slot < 0) return [...tabs, tab]
  return tabs.map((t, i) => (i === slot ? tab : t))
}

/** Promote a preview tab to one that stays. */
export function keepTab(tabs: Tab[], id: string): Tab[] {
  const found = tabs.find((t) => t.id === id)
  if (!found?.preview) return tabs
  return tabs.map((t) => (t.id === id ? { ...t, preview: false } : t))
}

/** Close a tab. Every tab can be closed, including the library. */
export function closeTab(tabs: Tab[], id: string): Tab[] {
  return tabs.filter((t) => t.id !== id)
}

/**
 * Which tab to show after closing `id`.
 *
 * The neighbour to the right, falling back to the left: closing a run of tabs
 * left to right then keeps the hand in one place, which is what every editor
 * does and what muscle memory expects.
 */
export function nextActive(tabs: Tab[], id: string, active: string): string {
  if (active !== id) return active
  const index = tabs.findIndex((t) => t.id === id)
  if (index < 0) return active
  const remaining = tabs.filter((t) => t.id !== id)
  // Nothing left is a real state, not a reason to conjure a tab back.
  if (!remaining.length) return ''
  return (remaining[index] ?? remaining[remaining.length - 1]!).id
}

/** Close every tab. */
export function closeAll(_tabs: Tab[]): Tab[] {
  return []
}

/** Close everything except one tab. */
export function closeOthers(tabs: Tab[], keep: string): Tab[] {
  return tabs.filter((t) => t.id === keep)
}
