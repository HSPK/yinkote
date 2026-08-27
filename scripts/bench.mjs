#!/usr/bin/env node
/**
 * Load generator and latency benchmark.
 *
 * Usage: node scripts/bench.mjs [base] [count]
 *
 * Seeds a synthetic library through the public API (the same path the UI uses,
 * so the numbers mean something) and then measures p50/p95/p99 for the queries
 * that sit in the interactive path.
 */

const BASE = (process.argv[2] ?? 'http://127.0.0.1:23130') + '/api/v1'
const TARGET = Number(process.argv[3] ?? 100_000)
const BATCH = 500
const CONCURRENCY = 8

const WORDS = [
  'diffusion', 'transformer', 'attention', 'retrieval', 'graph', 'protein',
  'quantum', 'federated', 'contrastive', 'multimodal', 'reinforcement',
  'segmentation', 'benchmark', 'survey', 'scaling', 'alignment', 'inference',
  'sparse', 'distillation', 'embedding', 'tokenizer', 'curriculum',
]
const CJK = ['扩散模型', '大语言模型', '知识图谱', '分子生成', '强化学习', '综述', '注意力机制', '多模态']
const SURNAMES = ['Vaswani', 'Ho', 'Zhang', 'Smith', 'Kim', 'Müller', 'Rossi', 'Ivanov', 'Chen', 'Dubois']
const VENUES = ['NeurIPS', 'ICML', 'ICLR', 'Nature', 'Science', 'JMLR', 'ACL', 'CVPR']
const TYPES = ['journalArticle', 'conferencePaper', 'preprint', 'book', 'thesis', 'report']

// Deterministic PRNG so runs are comparable.
let rngState = 0x2545f491
const rnd = () => {
  rngState ^= rngState << 13
  rngState ^= rngState >>> 17
  rngState ^= rngState << 5
  return ((rngState >>> 0) % 1_000_000) / 1_000_000
}
const pick = (arr) => arr[Math.floor(rnd() * arr.length)]

function makeItem(i) {
  const cjk = i % 5 === 0
  const title = cjk
    ? `${pick(CJK)}${pick(CJK)}研究 ${i}`
    : `${pick(WORDS)} ${pick(WORDS)} for ${pick(WORDS)} ${i}`
  return {
    itemType: pick(TYPES),
    title,
    abstractNote: cjk
      ? `本文提出了一种${pick(CJK)}方法，在${pick(CJK)}任务上取得了显著提升。编号 ${i}。`
      : `We present a ${pick(WORDS)} approach that improves ${pick(WORDS)} by a large margin (#${i}).`,
    date: `${2010 + (i % 16)}-0${1 + (i % 9)}-1${i % 10}`,
    publicationTitle: pick(VENUES),
    DOI: `10.9999/bench.${i}`,
    creators: [
      { creatorType: 'author', firstName: 'A', lastName: pick(SURNAMES) },
      { creatorType: 'author', firstName: 'B', lastName: pick(SURNAMES) },
    ],
    tags: [{ tag: pick(WORDS) }, { tag: pick(CJK) }],
  }
}

async function post(path, body) {
  const res = await fetch(BASE + path, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(body),
  })
  if (!res.ok) throw new Error(`${path} -> ${res.status} ${await res.text()}`)
  return res.json()
}

async function get(path) {
  const res = await fetch(BASE + path)
  if (!res.ok) throw new Error(`${path} -> ${res.status}`)
  return res.json()
}

function stats(samples) {
  const s = [...samples].sort((a, b) => a - b)
  const at = (q) => s[Math.min(s.length - 1, Math.floor(s.length * q))]
  return { p50: at(0.5), p95: at(0.95), p99: at(0.99), max: s.at(-1) }
}

async function measure(label, path, runs = 40) {
  const samples = []
  let count = 0
  for (let i = 0; i < runs; i++) {
    const t = performance.now()
    const body = await get(path.replace('%i', String(i)))
    samples.push(performance.now() - t)
    count = body.total ?? body.hits?.length ?? 0
  }
  const s = stats(samples)
  console.log(
    `  ${label.padEnd(34)} p50 ${s.p50.toFixed(1).padStart(6)}ms  ` +
      `p95 ${s.p95.toFixed(1).padStart(6)}ms  p99 ${s.p99.toFixed(1).padStart(6)}ms  ` +
      `(n=${count})`,
  )
  return s
}

async function seed(lib) {
  const existing = (await get(`/libraries/${lib}/items?limit=1`)).total
  if (existing >= TARGET) {
    console.log(`▸ library already holds ${existing} items, skipping seed`)
    return
  }
  const todo = TARGET - existing
  console.log(`▸ seeding ${todo} items (batch=${BATCH}, concurrency=${CONCURRENCY})`)

  const started = performance.now()
  let done = 0
  const batches = Math.ceil(todo / BATCH)
  let next = 0

  const worker = async () => {
    for (;;) {
      const b = next++
      if (b >= batches) return
      const items = Array.from({ length: Math.min(BATCH, todo - b * BATCH) }, (_, k) =>
        makeItem(existing + b * BATCH + k),
      )
      await post(`/libraries/${lib}/items`, items)
      done += items.length
      if (b % 20 === 0) {
        const rate = done / ((performance.now() - started) / 1000)
        process.stdout.write(`\r  ${done}/${todo}  ${rate.toFixed(0)} items/s   `)
      }
    }
  }
  await Promise.all(Array.from({ length: CONCURRENCY }, worker))
  const secs = (performance.now() - started) / 1000
  console.log(`\r  seeded ${todo} items in ${secs.toFixed(1)}s (${(todo / secs).toFixed(0)} items/s)`)
}

async function main() {
  const ping = await get('/ping')
  const lib = ping.defaultLibrary
  console.log(`▸ yinkote ${ping.version}, library ${lib}\n`)

  await seed(lib)

  const stat0 = await get('/stats')
  console.log(
    `\n▸ corpus: ${stat0.items} items, ${stat0.tags} tags, ` +
      `${stat0.search.embedded}/${stat0.search.documents} embedded (${stat0.search.provider})\n`,
  )

  console.log('▸ browse')
  await measure('list first page (sort=modified)', `/libraries/${lib}/items?limit=100`)
  await measure('list deep page (offset=50000)', `/libraries/${lib}/items?limit=100&offset=50000`)
  await measure('list sorted by title', `/libraries/${lib}/items?limit=100&sort=title`)
  await measure('list filtered by tag', `/libraries/${lib}/items?limit=100&tag=survey`)
  await measure('facets', `/libraries/${lib}/facets?limit=60`)

  console.log('\n▸ search')
  await measure('keyword (1 term)', `/libraries/${lib}/search?q=transformer&mode=keyword`)
  await measure('keyword (2 terms)', `/libraries/${lib}/search?q=diffusion%20alignment&mode=keyword`)
  await measure('chinese keyword', `/libraries/${lib}/search?q=%E6%89%A9%E6%95%A3%E6%A8%A1%E5%9E%8B&mode=keyword`)
  await measure('fuzzy (typo)', `/libraries/${lib}/search?q=transfromer&mode=fuzzy`)
  await measure('semantic', `/libraries/${lib}/search?q=generative%20model%20for%20molecules&mode=semantic`)
  await measure('hybrid', `/libraries/${lib}/search?q=diffusion%20model&mode=hybrid`)
  await measure('tag operator', `/libraries/${lib}/search?q=tag:survey`)
  await measure('hybrid + hydrate items', `/libraries/${lib}/items?q=attention&limit=50`)

  // The graph and citation endpoints joined the interactive path after these
  // numbers were first taken, and an endpoint nobody measures is an endpoint
  // that quietly gets slow.
  console.log('\n▸ relationships')
  const sample = await get(`/libraries/${lib}/items?limit=1`)
  const key = sample.items?.[0]?.key
  if (key) {
    await measure('graph neighbourhood', `/libraries/${lib}/graph/${key}`, 20)
  }

  console.log('\n▸ files')
  await measure('file browser page', `/libraries/${lib}/files?limit=500`, 20)
  {
    // Measured for its *size* as much as its speed: this once returned every
    // planned rename — 3.7 MB for a panel that shows eight lines.
    const t = performance.now()
    const response = await fetch(`${BASE}/libraries/${lib}/files/preview`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ template: '{author} {year} - {title}' }),
    })
    const text = await response.text()
    console.log(
      `  ${'rename preview'.padEnd(34)} ${(performance.now() - t).toFixed(1).padStart(6)}ms  ` +
        `${(text.length / 1024).toFixed(1)} KB`,
    )
  }

  console.log('\n▸ citations')
  if (key) {
    const runs = []
    for (let i = 0; i < 20; i++) {
      const t = performance.now()
      await post(`/libraries/${lib}/citations`, { keys: [key], style: 'apa' })
      runs.push(performance.now() - t)
    }
    const c = stats(runs)
    console.log(
      `  ${'render one reference'.padEnd(34)} p50 ${c.p50.toFixed(1).padStart(6)}ms  ` +
        `p95 ${c.p95.toFixed(1).padStart(6)}ms  p99 ${c.p99.toFixed(1).padStart(6)}ms`,
    )
  }

  console.log('\n▸ write')
  const writes = []
  for (let i = 0; i < 20; i++) {
    const t = performance.now()
    await post(`/libraries/${lib}/items`, [makeItem(900_000 + i)])
    writes.push(performance.now() - t)
  }
  const w = stats(writes)
  console.log(
    `  ${'single item create'.padEnd(34)} p50 ${w.p50.toFixed(1).padStart(6)}ms  ` +
      `p95 ${w.p95.toFixed(1).padStart(6)}ms  p99 ${w.p99.toFixed(1).padStart(6)}ms`,
  )

  const stat1 = await get('/stats')
  console.log(
    `\n▸ final: ${stat1.items} items, ${stat1.search.embedded} vectors, ` +
      `library version ${stat1.version}`,
  )
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
