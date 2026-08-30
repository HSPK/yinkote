import type { Creator, Item } from '../api/types'
import type { MessageKey, Translate } from '../i18n'

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

/**
 * What to call an item in a list.
 *
 * Annotations carry no title — the text is the point of a highlight — so they
 * rendered as "Untitled" wherever one appeared, which a search does whenever
 * it matches something the reader marked. Their own words are the only useful
 * label they have.
 *
 * Takes the fallback rather than translating here, so this stays free of the
 * i18n context and can be used from anywhere.
 */
export function displayTitle(item: Item, untitled: string): string {
  const title = item.title
  if (typeof title === 'string' && title.trim()) return title
  const marked = (item as { annotationText?: unknown }).annotationText
  if (typeof marked === 'string' && marked.trim()) return marked.trim()
  return untitled
}

/**
 * A task's progress line, translated.
 *
 * The server sends a code — `task.importingItems` — and this turns it into a
 * sentence. It used to send the sentence itself, in English, and four surfaces
 * printed it verbatim: the status bar, the jobs page, the activity indicator
 * and the import panels. A product whose rule is that every user-visible
 * string comes from a catalogue was showing a Chinese reader "Importing
 * items".
 *
 * Anything that is not a known code is returned unchanged, so a task recorded
 * by an older server still reads as words rather than as a bare key.
 */
export function taskMessage(t: Translate, message: string): string {
  return TASK_MESSAGES.has(message) ? t(message as MessageKey) : message
}

const TASK_MESSAGES = new Set([
  'task.readingClosely',
  'task.readingZotero',
  'task.reindexing',
  'task.summarising',
  'task.backingUp',
  'task.packing',
  'task.readingArchive',
  'task.fetchingReferences',
  'task.filingCollections',
  'task.importingAnnotations',
  'task.importingFiles',
  'task.importingItems',
  'task.importingNotes',
  'task.restoringFiles',
  'task.restoringItems',
  'task.writingItems',
])

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
  // A count that never arrived reads as "—", not "NaNM". Every surface shows
  // one of these beside a label, and a formatter is the wrong place to make
  // a missing number look like a very large one.
  if (!Number.isFinite(n)) return '—'
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

/** How long something has been going, at the coarsest useful resolution.
 *
 *  A turn that has been running for two seconds and one that has been running
 *  for two minutes need different reassurance, so the unit follows the
 *  magnitude: seconds while it is quick, minutes and seconds once it is not,
 *  hours when something has clearly gone wrong. Seconds are dropped past an
 *  hour, where they are noise.
 *
 *  Digits and unit letters rather than words, because this ticks once a second
 *  beside a spinner and must not change width every time.
 */
export function elapsed(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return '0s'
  const total = Math.floor(ms / 1000)
  const s = total % 60
  const m = Math.floor(total / 60) % 60
  const h = Math.floor(total / 3600)

  if (h) return `${h}h ${String(m).padStart(2, '0')}m`
  if (m) return `${m}m ${String(s).padStart(2, '0')}s`
  return `${s}s`
}

/** The modifier key this machine actually uses, for labels that name it.
 *
 *  The handlers all accept `metaKey || ctrlKey`, so both platforms work — but
 *  the empty-library hint said "press ⌘K" to everybody, which is a Mac symbol
 *  shown to Linux and Windows users of a program whose whole point is being
 *  cross-platform. Detected rather than configured: nobody should have to tell
 *  a local application which keyboard is in front of them.
 */
export function modKey(): string {
  const platform =
    (typeof navigator === 'undefined' ? '' : navigator.platform || navigator.userAgent) ?? ''
  return /Mac|iPhone|iPad/i.test(platform) ? '⌘' : 'Ctrl+'
}

/**
 *  Why a turn ended badly, in the reader's language.
 *
 *  The server's message is written in English where it is thrown and often
 *  carries the upstream service's raw JSON — and a throttled model is by far
 *  the most common failure here, so that is the one a reader meets. Unknown
 *  kinds return empty, and the caller falls back to the sentence: a sentence
 *  in the wrong language still beats a bare key.
 */
export function agentProblem(t: Translate, problem?: string): string {
  return problem && AGENT_PROBLEMS.has(problem) ? t(`agent.${problem}` as MessageKey) : ''
}

const AGENT_PROBLEMS = new Set([
  'rateLimited',
  'notConfigured',
  'timedOut',
  'unreachable',
  'refused',
  'failed',
])

/**
 *  What an embedding provider means for the results, not just its name.
 *
 *  `local-hash` was shown as a bare token in two places and nowhere explained.
 *  It is a hashed n-gram projection: it matches lexical and morphological
 *  similarity and has no semantic content at all, so a query sharing no words
 *  with a paper does not find it — measured, on "a model with no recurrence",
 *  which returned three unrelated papers with no indication that it had failed
 *  to understand the question.
 *
 *  That is a reasonable default — a feature that only works after signing up
 *  for an API key is a feature most people never see — but it has to be said,
 *  because "semantic" is a promise and this does not keep all of it.
 */
export function embedderMeaning(t: Translate, provider?: string): string {
  if (!provider) return ''
  return provider === 'local-hash' ? t('embedder.localHash') : t('embedder.remote')
}
