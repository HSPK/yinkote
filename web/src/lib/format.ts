import type { Creator, Item } from '../api/types'

export function creatorName(c: Creator): string {
  if (c.name) return c.name
  return [c.firstName, c.lastName].filter(Boolean).join(' ')
}

/** "Vaswani", "Vaswani & Shazeer", "Vaswani et al." */
export function creatorSummary(item: Item): string {
  const names = (item.creators ?? []).map((c) => c.lastName || c.name || c.firstName || '')
  if (names.length === 0) return ''
  if (names.length === 1) return names[0] ?? ''
  if (names.length === 2) return `${names[0]} & ${names[1]}`
  return `${names[0]} et al.`
}

export function year(item: Item): string {
  const m = /\d{4}/.exec(String(item.date ?? ''))
  return m ? m[0] : ''
}

export function shortDate(ms: number): string {
  if (!ms) return ''
  const d = new Date(ms)
  const now = new Date()
  const p = (n: number) => String(n).padStart(2, '0')
  return d.getFullYear() === now.getFullYear()
    ? `${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`
    : `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

/** Splits the server's `<mark>` snippet so React can render it without
 *  `dangerouslySetInnerHTML`. */
export function snippetParts(snippet: string): { text: string; mark: boolean }[] {
  const parts: { text: string; mark: boolean }[] = []
  const re = /<mark>([\s\S]*?)<\/mark>/g
  let last = 0
  let m: RegExpExecArray | null
  while ((m = re.exec(snippet)) !== null) {
    if (m.index > last) parts.push({ text: snippet.slice(last, m.index), mark: false })
    parts.push({ text: m[1] ?? '', mark: true })
    last = m.index + m[0].length
  }
  if (last < snippet.length) parts.push({ text: snippet.slice(last), mark: false })
  return parts
}

export function compact(n: number): string {
  if (n < 1000) return String(n)
  if (n < 1_000_000) return `${(n / 1000).toFixed(n < 10_000 ? 1 : 0)}k`
  return `${(n / 1_000_000).toFixed(1)}M`
}

/** A file size a person can read at a glance. */
export function bytes(n: number): string {
  if (!n) return ''
  const units = ['B', 'KB', 'MB', 'GB']
  let value = n
  let unit = 0
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024
    unit += 1
  }
  // One decimal below 10, none above: `4.2 MB` is useful, `412.7 KB` is noise.
  return `${value < 10 && unit > 0 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`
}
