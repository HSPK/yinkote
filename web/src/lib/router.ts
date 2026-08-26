/** Which top-level view is showing.
 *
 *  Backed by `location.hash` rather than component state so the browser Back
 *  button works and a page can be linked to — this is a web app, and behaving
 *  like one is free.
 */

export const PAGES = ['library', 'chat', 'plugins', 'status', 'settings'] as const
export type Page = (typeof PAGES)[number]

export const PAGE_LABELS: Record<Page, { label: string; glyph: string }> = {
  library: { label: '文库', glyph: '▤' },
  chat: { label: '对话', glyph: '✦' },
  plugins: { label: '插件', glyph: '⊞' },
  status: { label: '状态', glyph: '◱' },
  settings: { label: '设置', glyph: '⚙' },
}

const DEFAULT: Page = 'library'

export function isPage(value: string): value is Page {
  return (PAGES as readonly string[]).includes(value)
}

export function pageFromHash(hash: string = location.hash): Page {
  const name = hash.replace(/^#\/?/, '').split('?')[0] ?? ''
  return isPage(name) ? name : DEFAULT
}

export function navigate(page: Page): void {
  if (pageFromHash() !== page) location.hash = `#/${page}`
}

/** Subscribe to hash changes. Returns an unsubscribe function. */
export function onNavigate(handler: (page: Page) => void): () => void {
  const listener = () => handler(pageFromHash())
  window.addEventListener('hashchange', listener)
  return () => window.removeEventListener('hashchange', listener)
}
