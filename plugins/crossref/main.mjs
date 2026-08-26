/**
 * Crossref metadata source.
 *
 * Demonstrates the whole plugin surface in ~150 lines:
 *   - the `initialize` handshake and capability registration
 *   - custom methods invoked via POST /api/v1/plugins/crossref/call
 *   - calling back into the host (`host.log`, `host.items.create`)
 *
 * Protocol: newline-delimited JSON-RPC 2.0 on stdin/stdout. stderr is captured
 * as log output, so never write protocol data there.
 */
import { createInterface } from 'node:readline'

const API = 'https://api.crossref.org/works'
const UA = 'Yinkote/0.1 (https://github.com/yinkote/yinkote)'

// ── protocol plumbing ───────────────────────────────────────────────────────

let nextId = 1
const pending = new Map()

function send(message) {
  process.stdout.write(JSON.stringify(message) + '\n')
}

/** Call a host method and await its reply. */
function callHost(method, params = {}) {
  const id = 100000 + nextId++
  send({ jsonrpc: '2.0', id, method, params })
  return new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending.delete(id)
      reject(new Error(`host call '${method}' timed out`))
    }, 10000)
    pending.set(id, { resolve, reject, timer })
  })
}

const log = (message) => callHost('host.log', { level: 'info', message }).catch(() => {})

// ── Crossref mapping ────────────────────────────────────────────────────────

const TYPE_MAP = {
  'journal-article': 'journalArticle',
  'proceedings-article': 'conferencePaper',
  'posted-content': 'preprint',
  book: 'book',
  'book-chapter': 'bookSection',
  dissertation: 'thesis',
  report: 'report',
  dataset: 'dataset',
  standard: 'standard',
}

function creators(work) {
  return (work.author ?? []).map((a) => ({
    creatorType: 'author',
    ...(a.family ? { firstName: a.given ?? '', lastName: a.family } : { name: a.name ?? '' }),
  }))
}

function isoDate(work) {
  const parts = work.issued?.['date-parts']?.[0]
  if (!Array.isArray(parts) || parts.length === 0) return ''
  return parts.map((n, i) => (i === 0 ? String(n) : String(n).padStart(2, '0'))).join('-')
}

/** Crossref work → Yinkote item draft. */
function toDraft(work) {
  const draft = {
    itemType: TYPE_MAP[work.type] ?? 'journalArticle',
    title: (work.title ?? [])[0] ?? '(untitled)',
    date: isoDate(work),
    creators: creators(work),
    tags: (work.subject ?? []).slice(0, 6).map((s) => ({ tag: s, type: 1 })),
  }
  const set = (key, value) => {
    if (value !== undefined && value !== null && value !== '') draft[key] = String(value)
  }
  set('DOI', work.DOI)
  set('url', work.URL)
  set('publicationTitle', (work['container-title'] ?? [])[0])
  set('publisher', work.publisher)
  set('volume', work.volume)
  set('issue', work.issue)
  set('pages', work.page)
  set('ISSN', (work.ISSN ?? [])[0])
  set('ISBN', (work.ISBN ?? [])[0])
  set('language', work.language)
  set('abstractNote', work.abstract?.replace(/<[^>]+>/g, '').trim())
  return draft
}

async function crossref(path, params) {
  const url = new URL(API + path)
  for (const [k, v] of Object.entries(params)) url.searchParams.set(k, String(v))
  const res = await fetch(url, { headers: { 'User-Agent': UA } })
  if (!res.ok) throw new Error(`Crossref responded ${res.status}`)
  return res.json()
}

const DOI_RE = /\b(10\.\d{4,9}\/[^\s"'<>]+)\b/i

// ── methods ─────────────────────────────────────────────────────────────────

const methods = {
  initialize: () => ({
    contributions: {
      metadataSources: [
        {
          id: 'crossref',
          label: 'Crossref',
          description: '1.5 亿+ 学术文献的权威 DOI 元数据',
          supports: ['query', 'doi'],
        },
      ],
    },
  }),

  /** { text, limit } → { items: draft[] } */
  async search({ text = '', limit = 10 } = {}) {
    const doi = DOI_RE.exec(text)?.[1]
    if (doi) {
      const body = await crossref(`/${encodeURIComponent(doi)}`, {})
      return { items: [toDraft(body.message)] }
    }
    const body = await crossref('', { 'query.bibliographic': text, rows: Math.min(limit, 20) })
    return { items: (body.message?.items ?? []).map(toDraft) }
  },

  /** { text, limit, libraryId } → imports straight into the library. */
  async import({ text, limit = 5, libraryId } = {}) {
    const { items } = await methods.search({ text, limit })
    if (items.length === 0) return { created: [], failed: 0 }
    await log(`importing ${items.length} result(s) for "${text}"`)
    return callHost('host.items.create', { items, libraryId })
  },

  hook: () => ({}),
  shutdown: () => null,
}

// ── main loop ───────────────────────────────────────────────────────────────

const rl = createInterface({ input: process.stdin })

rl.on('line', async (line) => {
  if (!line.trim()) return
  let message
  try {
    message = JSON.parse(line)
  } catch {
    return
  }

  // Replies to our own host calls.
  if (message.method === undefined && pending.has(message.id)) {
    const entry = pending.get(message.id)
    pending.delete(message.id)
    clearTimeout(entry.timer)
    if (message.error) entry.reject(new Error(message.error.message))
    else entry.resolve(message.result)
    return
  }

  const handler = methods[message.method]
  if (!handler) {
    send({
      jsonrpc: '2.0',
      id: message.id,
      error: { code: -32601, message: `unknown method '${message.method}'` },
    })
    return
  }

  try {
    const result = await handler(message.params ?? {})
    send({ jsonrpc: '2.0', id: message.id, result: result ?? null })
    if (message.method === 'shutdown') process.exit(0)
  } catch (err) {
    send({
      jsonrpc: '2.0',
      id: message.id,
      error: { code: -32000, message: String(err?.message ?? err) },
    })
  }
})

rl.on('close', () => process.exit(0))
