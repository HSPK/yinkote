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

// Honour `YK_PORT`, because that is what the runbook sets and a script that
// silently ignores the variable you gave it will happily measure the wrong
// server for weeks. This one did: every `YK_PORT=23140 node bench.mjs` was
// benchmarking the smoke database on 23130, and seeded its collections there.
const PORT = process.env.YK_PORT ?? '23130'
const BASE = (process.argv[2] ?? `http://127.0.0.1:${PORT}`) + '/api/v1'
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
    tags: tagsFor(i),
  }
}

/** Tags for one item, distributed the way a real library's are.
 *
 *  Every item carrying one of thirty broad words is not a library, it is a
 *  category system — and it made the benchmark pessimistic in two places at
 *  once: a "shared tag" spanned thousands of items, so the graph aggregated
 *  tens of thousands of rows, and a tag filter matched a fifth of the corpus.
 *
 *  Real tagging has a long tail: a handful of words on a lot of papers, and
 *  a great many project-specific tags on two or three each. That is what makes
 *  "papers sharing this tag" a small set, which is what the feature is for.
 */
function tagsFor(i) {
  const tags = []
  // A broad one, on roughly a twentieth of the library.
  if (i % 20 === 0) tags.push({ tag: pick(WORDS) })
  if (i % 37 === 0) tags.push({ tag: pick(CJK) })
  // And the tail: a tag shared by a handful of neighbouring items, which is
  // what somebody filing a reading list actually produces.
  tags.push({ tag: `project-${Math.floor(i / 7)}` })
  if (i % 3 === 0) tags.push({ tag: `topic-${Math.floor(i / 23)}` })
  return tags
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

/** What a measurement is allowed to cost, in p50 milliseconds.
 *
 *  Only for the numbers where a regression means something went structurally
 *  wrong rather than a machine being busy — a browse that stops using its index
 *  goes from 9ms to 69ms, which no amount of noise explains. Set well above the
 *  measured figure so an ordinary bad day does not fail the run.
 *
 *  This exists because that regression shipped: the plan assertions could not
 *  see it (an empty test database picks the right index whatever the statement
 *  says) and the benchmark printed 68.6ms next to a comment saying 9ms, and
 *  printing is not checking.
 */
const BUDGET = {
  // 8.3ms before the listing total was cached against the library version,
  // 4.4 after — counting was most of a browse. Held at 15 rather than 6: the
  // job of these numbers is to catch a plan collapsing (9ms to 69ms when an
  // index stopped being named), and a threshold close to the measurement fails
  // for machine reasons instead.
  'list first page (sort=modified)': 15,
  'list deep page (offset=50000)': 15,
  'list sorted by title': 15,
  // 36.2ms before the plan was chosen from the tag's cardinality, 17.3 after,
  // and nearly all of what is left is the count rather than the page. A
  // regression to materialising every tag would land back above this.
  'list filtered by tag': 28,
  // 313.7ms before a selection was fetched by key instead of filtered out of
  // the whole library, 1.5ms after. This is the path the UI uses.
  'rename preview (selection)': 15,
  // 32.5ms while a pure filter query read 20,000 ids to return 50, and while
  // it materialised the tag set instead of walking the sort order. 2.5ms once
  // it read what it needed and chose its plan.
  'list filtered by rare tag': 8,
  'tag operator': 10,
  'list one collection': 25,
  'list collection + children': 25,
  facets: 15,
  stats: 30,
}

const overBudget = []

/** Print one line and hold it to its budget.
 *
 *  Separate from `measure` because not every measurement is a plain GET — a
 *  preview is a POST with a body — and the budget is the point of the whole
 *  script. A measurement that prints through its own format string is a
 *  measurement nothing is checking.
 */
function report(label, s, count) {
  const budget = BUDGET[label]
  const over = budget !== undefined && s.p50 > budget
  if (over) overBudget.push(`${label}: ${s.p50.toFixed(1)}ms, budget ${budget}ms`)
  console.log(
    `  ${label.padEnd(34)} p50 ${s.p50.toFixed(1).padStart(6)}ms  ` +
      `p95 ${s.p95.toFixed(1).padStart(6)}ms  p99 ${s.p99.toFixed(1).padStart(6)}ms  ` +
      `(n=${count})${over ? '  ← OVER BUDGET' : ''}`,
  )
  return s
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
  // The first sample is kept separate: for anything the server warms at
  // startup, a broken warm-up shows up only there — the other 39 calls warm
  // each other and the median says nothing.
  return { ...report(label, stats(samples), count), first: samples[0] }
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

/** How many collections a real shelf has, and how deep it nests. */
const COLLECTIONS = 40
const NESTED = 12
/** Share of the library filed somewhere. Most people file most things. */
const FILED_SHARE = 0.6
/** Items per membership call. */
const FILE_BATCH = 500

/** Give the corpus the shape a real library has.
 *
 *  A hundred thousand items in *no collections* is not a library, it is a
 *  list — and it hid a real defect for months: the statistics endpoint counted
 *  collections by listing them, which attaches a membership count to each and
 *  costs a pass over `collection_items`. With no memberships to walk it
 *  measured 0.33ms and looked fine; at a hundred thousand it is about 29ms.
 *
 *  A benchmark that cannot see a defect is worse than none, because it
 *  certifies performance the program does not have.
 */
async function seedShelves(lib) {
  const existing = await get(`/libraries/${lib}/collections`)
  if (existing.length >= COLLECTIONS) {
    console.log(`▸ library already has ${existing.length} collections, skipping`)
    return existing
  }

  const started = performance.now()
  const made = []
  for (let i = 0; i < COLLECTIONS; i++) {
    // Some nested, because a flat list of forty is not what anybody's
    // shelves look like and recursive queries are the interesting ones.
    const parent = i >= COLLECTIONS - NESTED ? made[i % (COLLECTIONS - NESTED)]?.key : undefined
    made.push(await post(`/libraries/${lib}/collections`, { name: `Shelf ${i}`, parent }))
  }

  const total = (await get(`/libraries/${lib}/items?limit=1`)).total
  const toFile = Math.floor(total * FILED_SHARE)
  let filed = 0
  for (let offset = 0; filed < toFile; offset += FILE_BATCH) {
    const page = await get(`/libraries/${lib}/items?limit=${FILE_BATCH}&offset=${offset}`)
    if (!page.items.length) break
    const keys = page.items.map((i) => i.key)
    const shelf = made[Math.floor(offset / FILE_BATCH) % made.length]
    await post(`/libraries/${lib}/collections/${shelf.key}/items`, { keys })
    filed += keys.length
    if (offset % (FILE_BATCH * 20) === 0) {
      process.stdout.write(`\r  filing ${filed}/${toFile}   `)
    }
  }
  const secs = (performance.now() - started) / 1000
  console.log(`\r  filed ${filed} items into ${made.length} shelves in ${secs.toFixed(1)}s`)
  return made
}

async function main() {
  const ping = await get('/ping')
  const lib = ping.defaultLibrary
  // The address *and* the database behind it. Printing the address was not
  // enough: a server that failed to start leaves the port held by an older one,
  // which answers happily and is a different library. That has now happened
  // twice — once measuring the smoke database for weeks, once seeding shelves
  // into somebody else's corpus.
  console.log(`▸ yinkote ${ping.version}, library ${lib}, at ${BASE}`)
  console.log(`▸ database: ${ping.dataDir}\n`)

  if (process.env.YK_DATA && ping.dataDir !== process.env.YK_DATA) {
    // Refusing rather than warning: this run would seed a hundred thousand
    // items into a library nobody meant to touch.
    console.error(
      `▸ refusing to run: expected ${process.env.YK_DATA}, found ${ping.dataDir}.\n` +
        `  Something else is already listening on this port.`,
    )
    process.exit(2)
  }

  await seed(lib)
  const shelves = await seedShelves(lib)

  // Give the planner the statistics a real library has. A database that has
  // been in use accumulates `sqlite_stat1`/`stat4` through `PRAGMA optimize`,
  // and SQLite chooses differently with them than without — a freshly seeded
  // corpus is a different planner, so measuring one and comparing it to the
  // other compares two things. Collection pages moved 22.6ms -> 14.6ms on this
  // corpus purely from running it.
  await fetch(`${BASE}/maintenance/optimize`, { method: 'POST' }).catch(() => {})

  const stat0 = await get('/stats')
  console.log(
    `\n▸ corpus: ${stat0.items} items, ${stat0.tags} tags, ` +
      `${stat0.collections} collections, ` +
      `${stat0.search.embedded}/${stat0.search.documents} embedded (${stat0.search.provider})\n`,
  )

  console.log('▸ browse')
  await measure('list first page (sort=modified)', `/libraries/${lib}/items?limit=100`)
  await measure('list deep page (offset=50000)', `/libraries/${lib}/items?limit=100&offset=50000`)
  await measure('list sorted by title', `/libraries/${lib}/items?limit=100&sort=title`)
  await measure('list filtered by tag', `/libraries/${lib}/items?limit=100&tag=survey`)
  // The other side of the same decision. Which SQL a tag filter becomes turns
  // on how common the tag is, so measuring only a common one leaves half the
  // choice unwatched — and that half is where forcing the sort index onto a
  // materialised filter cost 131x (see docs/16 3.122).
  //
  // Discovered rather than named: a hard-coded tag matched nothing on this
  // corpus and the line measured an empty result set, which is the shape of a
  // check that passes for the wrong reason.
  const facets = await get(`/libraries/${lib}/facets?limit=2000`)
  const rare = [...facets].sort((a, b) => a.count - b.count).find((t) => t.count > 0)
  if (rare) {
    console.log(`  (rarest tag: ${rare.name}, ${rare.count} items)`)
    await measure(
      'list filtered by rare tag',
      `/libraries/${lib}/items?limit=100&tag=${encodeURIComponent(rare.name)}`,
    )
  }
  // No `limit`, because the sidebar sends none and the startup warm-up
  // computes that exact slot. Asking for 60 measured a cache nobody fills:
  // 232ms on the first call and 1.9ms after, for a path the product never
  // takes — reporting a number three orders out from what a user sees, and
  // leaving the warm-up with no coverage at all. Third time this shape has
  // bitten; see docs/16 3.48, 3.49 and 3.126.
  const facetTiming = await measure('facets', `/libraries/${lib}/facets`)
  // 227ms cold against 2ms warm, which is why `warm_first_load` exists. By the
  // time the benchmark runs, startup is long over, so the first call here is
  // warm *if the warm-up worked* — and that is the only sample that can tell.
  if (facetTiming.first > 40) {
    overBudget.push(`facets first call: ${facetTiming.first.toFixed(1)}ms — startup warm-up missed it`)
    console.log(`  ${' '.repeat(34)} first ${facetTiming.first.toFixed(1)}ms  ← COLD`)
  }
  // Browsing a shelf, which is how most people navigate a library and which
  // went entirely unmeasured while the corpus had no collections in it.
  const shelf = shelves[0]?.key
  if (shelf) {
    await measure('list one collection', `/libraries/${lib}/items?limit=100&collection=${shelf}`)
    await measure(
      'list collection + children',
      `/libraries/${lib}/items?limit=100&collection=${shelf}&recursive=true`,
    )
  }
  // The statistics the workbench asks for on every load.
  await measure('stats', '/stats')

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
  {
    // The case the UI actually uses: pick a few files and preview those. It
    // has to cost what the selection costs, not what the library costs —
    // planning all 30,000 to keep twelve was 314ms, and the discarding was a
    // linear scan of the selection per row.
    // Sliced here rather than trusted to the query: `/files` pages by its own
    // rule, and a benchmark whose input size is decided elsewhere measures
    // something different every time that rule changes.
    const files = await get(`/libraries/${lib}/files`)
    const keys = files.files.filter((f) => f.filename).map((f) => f.key).slice(0, 100)
    const samples = []
    for (let i = 0; i < 20; i++) {
      const t = performance.now()
      await fetch(`${BASE}/libraries/${lib}/files/preview`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ template: '{author} {year} - {title}', keys }),
      }).then((r) => r.text())
      samples.push(performance.now() - t)
    }
    const s = stats(samples)
    report('rename preview (selection)', s, keys.length)
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

  if (overBudget.length) {
    // Fail, rather than print. A benchmark that only prints is a benchmark
    // whose regressions are found by whoever happens to read carefully.
    console.error(`\n▸ ${overBudget.length} measurement(s) over budget:`)
    for (const line of overBudget) console.error(`  ${line}`)
    process.exitCode = 1
  }
}

main().catch((e) => {
  console.error(e)
  process.exit(1)
})
