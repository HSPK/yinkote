/**
 * Journal metrics badges: impact factor, JCR quartile, CAS tier.
 *
 * Where the numbers come from, in order:
 *
 * 1. `metrics.json` beside this file. Authoritative, and where a licensed JCR
 *    or CAS dataset goes -- those rankings are proprietary and can be neither
 *    computed from open data nor shipped here.
 * 2. `cache.json`: what OpenAlex has already been asked.
 * 3. OpenAlex, live, batched by ISSN.
 *
 * OpenAlex publishes a journal's two-year mean citedness, which is the same
 * shape of measure as Clarivate's Journal Impact Factor and is *not* that
 * number. The tooltip says so, because a badge reading "IF 5.7" that quietly
 * means something else is worse than an empty column -- it will be quoted.
 *
 * The bundled table used to be the only source and held eighteen journals, so
 * a real library showed nothing at all.
 */
import { createInterface } from 'node:readline'
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const CACHE_PATH = join(here, 'cache.json')

function load(path) {
  try {
    return JSON.parse(readFileSync(path, 'utf8'))
  } catch {
    return {}
  }
}

/** { issn|normalised journal name: { if, jcr, cas } } -- licensed or hand-kept. */
const TABLE = load(join(here, 'metrics.json'))
/** The same shape, plus `source`, for what OpenAlex has answered before. */
const CACHE = load(CACHE_PATH)

let cacheDirty = false
function rememberCache() {
  if (!cacheDirty) return
  try {
    writeFileSync(CACHE_PATH, JSON.stringify(CACHE, null, 1))
    cacheDirty = false
  } catch {
    // A read-only plugin directory is not a reason to stop answering; it only
    // means the next run asks again.
  }
}

const normalise = (s) => String(s ?? '').toLowerCase().replace(/[^a-z0-9]+/g, '')

/** Every ISSN on an item: records carry print and electronic, and a lookup
 *  that only tries the first misses whichever one the dataset used. */
function issnsOf(fields) {
  const raw = [fields?.ISSN, fields?.issn]
    .flatMap((v) => String(v ?? '').split(/[,;\s]+/))
    .map((s) => s.trim())
    .filter((s) => /^\d{4}-\d{3}[\dxX]$/.test(s))
  return [...new Set(raw)]
}

function lookup(fields) {
  for (const issn of issnsOf(fields)) {
    if (TABLE[issn]) return TABLE[issn]
  }
  const byName = TABLE[normalise(fields?.publicationTitle)]
  if (byName) return byName
  for (const issn of issnsOf(fields)) {
    if (CACHE[issn]) return CACHE[issn]
  }
  return CACHE[normalise(fields?.publicationTitle)] ?? null
}

/**
 * Ask OpenAlex about the journals we have not seen, in one request.
 *
 * Batched because a page of fifty papers is usually a handful of journals, and
 * fifty requests to answer five questions would be both slow and rude.
 */
async function fetchMissing(items) {
  const wanted = new Set()
  for (const item of items) {
    if (lookup(item?.fields)) continue
    for (const issn of issnsOf(item?.fields)) wanted.add(issn)
  }
  if (wanted.size === 0) return

  const issns = [...wanted].slice(0, 50)
  const url =
    `https://api.openalex.org/sources?per-page=50` +
    `&select=id,display_name,issn,summary_stats` +
    `&filter=issn:${issns.join('|')}` +
    `&mailto=yinkote@localhost`

  const controller = new AbortController()
  const timer = setTimeout(() => controller.abort(), 3500)
  try {
    const response = await fetch(url, { signal: controller.signal })
    if (!response.ok) return
    const body = await response.json()
    for (const source of body.results ?? []) {
      const metric = source.summary_stats?.['2yr_mean_citedness']
      if (typeof metric !== 'number') continue
      // The journal's own name travels with the number. A row whose
      // `publicationTitle` disagrees with it is a record with a wrong journal
      // on it, and that is worth being able to see: two papers here were
      // filed under invented journal names while their ISSN and DOI were
      // right, so a correct impact factor looked like a broken plugin.
      const entry = {
        if: Math.round(metric * 10) / 10,
        source: 'openalex',
        journal: source.display_name,
      }
      for (const issn of source.issn ?? []) CACHE[issn] = entry
      CACHE[normalise(source.display_name)] = entry
      cacheDirty = true
    }
    // Remember the ones it had nothing for, so they are not asked again on
    // every scroll: a null is an answer.
    for (const issn of issns) {
      if (!CACHE[issn]) {
        CACHE[issn] = { source: 'openalex' }
        cacheDirty = true
      }
    }
    rememberCache()
  } catch {
    // Offline, throttled, or slower than the host will wait. The badges stay
    // empty this time and the next page will try again.
  } finally {
    clearTimeout(timer)
  }
}

/** Impact factor colouring: the thresholds are conventional, not derived. */
function impactTone(value) {
  if (value >= 20) return 'violet'
  if (value >= 10) return 'red'
  if (value >= 5) return 'amber'
  if (value >= 2) return 'green'
  return 'blue'
}

/* Every level gets its own colour rather than three shades of one, so a
   quartile can be told apart at a glance without reading it. */
const QUARTILE_TONE = { Q1: 'red', Q2: 'amber', Q3: 'green', Q4: 'blue' }
const CAS_TONE = { 1: 'red', 2: 'amber', 3: 'green', 4: 'blue' }

/* Ranks are "higher is better", which is what the host sorts by. Quartiles and
   tiers count the other way round, so they are inverted here — the host has no
   way to know that Q1 beats Q4. */
const QUARTILE_RANK = { Q1: 4, Q2: 3, Q3: 2, Q4: 1 }

/** What a number means and where it came from, for the tooltip and the
 *  detail panel. Named so the reader can check it: an unattributed metric is
 *  one nobody can argue with. */
function describe(metrics) {
  const where = metrics.journal ? ` for ${metrics.journal}` : ''
  return metrics.source === 'openalex'
    ? `OpenAlex two-year mean citedness ${metrics.if}${where} — not the Clarivate JIF`
    : `Impact factor ${metrics.if}${where} — from metrics.json`
}

function badgesFor(item, wanted) {
  const metrics = lookup(item?.fields)
  if (!metrics) return []

  const out = []
  // Zero means "no citations recorded in the window", which is what OpenAlex
  // reports for a journal too new or too small to have any. Printing "0.0" in
  // a column headed IF reads as a measurement, and it is an absence.
  if (wanted.includes('if') && typeof metrics.if === 'number' && metrics.if > 0) {
    out.push({
      badge: 'if',
      text: metrics.if.toFixed(1),
      rank: metrics.if,
      tone: impactTone(metrics.if),
      // Says which measure this is. The two are close enough in spirit to be
      // confused and far enough apart to matter.
      title: describe(metrics),
    })
  }
  if (wanted.includes('jcr') && metrics.jcr) {
    out.push({
      badge: 'jcr',
      text: metrics.jcr,
      rank: QUARTILE_RANK[metrics.jcr] ?? 0,
      tone: QUARTILE_TONE[metrics.jcr] ?? 'neutral',
      title: `JCR ${metrics.jcr}`,
    })
  }
  if (wanted.includes('cas') && metrics.cas) {
    out.push({
      badge: 'cas',
      text: `${metrics.cas}区`,
      rank: 5 - metrics.cas,
      tone: CAS_TONE[metrics.cas] ?? 'neutral',
      title: `CAS tier ${metrics.cas}`,
    })
  }
  return out
}

function send(message) {
  process.stdout.write(JSON.stringify(message) + '\n')
}

const methods = {
  initialize: () => ({
    contributions: {
      badges: [
        {
          id: 'if',
          sortable: true,
          label: 'IF',
          description: 'Journal impact factor',
          needs: ['ISSN', 'publicationTitle'],
          width: 60,
        },
        {
          id: 'jcr',
          sortable: true,
          label: 'JCR',
          description: 'JCR quartile',
          needs: ['ISSN', 'publicationTitle'],
          width: 56,
        },
        {
          id: 'cas',
          sortable: true,
          label: 'CAS',
          description: 'Chinese Academy of Sciences journal tier',
          needs: ['ISSN', 'publicationTitle'],
          width: 56,
        },
      ],
    },
  }),

  'badges.resolve': async ({ badges = [], items = [] } = {}) => {
    // Only worth a lookup when an impact number was asked for: the quartile
    // and the tier come from a licensed table or not at all.
    if (badges.includes('if')) await fetchMissing(items)
    const out = {}
    for (const item of items) {
      const values = badgesFor(item, badges)
      if (values.length) out[item.key] = values
    }
    return { badges: out }
  },

  shutdown: () => null,
}

/** Requests still being answered.
 *
 *  The loop used to exit the moment stdin closed, which was safe while every
 *  handler was synchronous: the reply had already been written. An async one
 *  loses that race and the caller gets silence, so closing now waits for what
 *  is in flight.
 */
let inFlight = 0
let inputClosed = false
const exitWhenIdle = () => {
  if (inputClosed && inFlight === 0) process.exit(0)
}

createInterface({ input: process.stdin })
  .on('line', (line) => {
    if (!line.trim()) return
    let message
    try {
      message = JSON.parse(line)
    } catch {
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
    // Awaited, because resolving a badge may have to ask the network. The
    // loop used to call handlers synchronously, so an async one would have
    // replied with a pending promise and the host would have seen nothing.
    inFlight += 1
    Promise.resolve()
      .then(() => handler(message.params ?? {}))
      .then((result) => {
        send({ jsonrpc: '2.0', id: message.id, result: result ?? null })
        if (message.method === 'shutdown') process.exit(0)
      })
      .catch((err) => {
        send({
          jsonrpc: '2.0',
          id: message.id,
          error: { code: -32000, message: String(err?.message ?? err) },
        })
      })
      .finally(() => {
        inFlight -= 1
        exitWhenIdle()
      })
  })
  .on('close', () => {
    inputClosed = true
    exitWhenIdle()
  })
