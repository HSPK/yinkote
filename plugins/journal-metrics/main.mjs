/**
 * Journal metrics badges.
 *
 * Demonstrates the badge contribution point: the plugin declares which columns
 * it can fill and which item fields it needs, and the host sends nothing else.
 *
 * The bundled table is a stand-in. Real deployments replace `metrics.json`
 * with a licensed dataset; nothing else here changes, which is the point of
 * keeping this outside the host.
 */
import { createInterface } from 'node:readline'
import { readFileSync } from 'node:fs'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))

/** { issn|normalised journal name: { if, jcr, cas } } */
const TABLE = JSON.parse(readFileSync(join(here, 'metrics.json'), 'utf8'))

const normalise = (s) => String(s ?? '').toLowerCase().replace(/[^a-z0-9]+/g, '')

function lookup(fields) {
  // ISSN is exact; the journal name is a fallback for records that lack one.
  const issn = String(fields?.ISSN ?? '').trim()
  return TABLE[issn] ?? TABLE[normalise(fields?.publicationTitle)] ?? null
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

function badgesFor(item, wanted) {
  const metrics = lookup(item?.fields)
  if (!metrics) return []

  const out = []
  if (wanted.includes('if') && typeof metrics.if === 'number') {
    out.push({
      badge: 'if',
      text: metrics.if.toFixed(1),
      rank: metrics.if,
      tone: impactTone(metrics.if),
      title: `Impact factor ${metrics.if}`,
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

  'badges.resolve': ({ badges = [], items = [] } = {}) => {
    const out = {}
    for (const item of items) {
      const values = badgesFor(item, badges)
      if (values.length) out[item.key] = values
    }
    return { badges: out }
  },

  shutdown: () => null,
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
    try {
      send({ jsonrpc: '2.0', id: message.id, result: handler(message.params ?? {}) ?? null })
      if (message.method === 'shutdown') process.exit(0)
    } catch (err) {
      send({
        jsonrpc: '2.0',
        id: message.id,
        error: { code: -32000, message: String(err?.message ?? err) },
      })
    }
  })
  .on('close', () => process.exit(0))
