/* The Word task pane.
 *
 * Plain ES modules-free script on purpose: it is served straight from the
 * server and loaded by Word's embedded browser, so a build step here would buy
 * nothing and cost a toolchain.
 *
 * Where state lives, and why:
 *
 * - The *document* owns the citations. Each one is a Word content control; the
 *   citation behind it is kept in `Office.context.document.settings`, which is
 *   stored inside the .docx. Reopen the file on another machine and the
 *   citations still know what they cite, which is the whole reason the server
 *   holds no record of documents (see integration/mod.rs).
 * - The *server* owns the rendering. The pane never formats anything; it sends
 *   the whole field list and writes back the text it is given. Numeric styles
 *   are the reason: inserting [3] renumbers [4] and [5], so no client-side
 *   shortcut for "just this one citation" is ever correct.
 * - The *session* is a cache and may vanish. Every call that gets a 404 for a
 *   dead session reopens one and retries once.
 */

const API = '/api/v1'
const TAG = 'yinkote-citation'
const BIB_TAG = 'yinkote-bibliography'
const FIELDS_KEY = 'yinkote.fields'
const DOC_KEY = 'yinkote.docId'
const KEY_STORE = 'yinkote.apiKey'

const el = (id) => document.getElementById(id)
const state = { session: null, prefs: null, library: null, picked: null, results: [], busy: false }

/* ---------------------------------------------------------------- transport */

function authHeaders() {
  const key = (() => {
    try {
      return window.localStorage.getItem(KEY_STORE)
    } catch {
      return null // Word's webview can have storage disabled; the key is optional anyway.
    }
  })()
  return key ? { Authorization: `Bearer ${key}` } : {}
}

async function api(path, options = {}) {
  const response = await fetch(API + path, {
    ...options,
    headers: {
      ...(options.body ? { 'Content-Type': 'application/json' } : {}),
      ...authHeaders(),
      ...(options.headers || {}),
    },
  })
  if (!response.ok) {
    let detail = `${response.status}`
    try {
      const body = await response.json()
      detail = body.title || body.detail || detail
    } catch {
      /* a non-JSON error body is still an error */
    }
    const error = new Error(detail)
    error.status = response.status
    throw error
  }
  return response.status === 204 ? null : response.json()
}

/* ------------------------------------------------------------ document state */

function settings() {
  return Office.context.document.settings
}

function saveSettings() {
  return new Promise((resolve, reject) => {
    settings().saveAsync((result) =>
      result.status === Office.AsyncResultStatus.Succeeded ? resolve() : reject(result.error),
    )
  })
}

function docId() {
  let id = settings().get(DOC_KEY)
  if (!id) {
    id = `doc-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 10)}`
    settings().set(DOC_KEY, id)
  }
  return id
}

/** The citation behind each content control, keyed by the control's id. */
function fieldMap() {
  return settings().get(FIELDS_KEY) || {}
}

/* ------------------------------------------------------------------ session */

async function openSession() {
  const saved = settings().get('yinkote.prefs')
  const body = { docId: docId() }
  // Only send prefs the document actually carries. An absent `docPrefs` means
  // "you decide", and sending defaults instead would silently overwrite the
  // style of a document written on another machine.
  if (saved && saved.styleId) body.docPrefs = saved
  const opened = await api('/integration/session', { method: 'POST', body: JSON.stringify(body) })
  state.session = opened.sessionId
  state.prefs = opened.prefs
  settings().set('yinkote.prefs', opened.prefs)
  await saveSettings()
  return opened
}

/** Call a session endpoint, reopening once if the server forgot the session. */
async function session(path, body) {
  if (!state.session) await openSession()
  const call = () =>
    api(`/integration/session/${state.session}${path}`, {
      method: path === '/prefs' ? 'PUT' : 'POST',
      body: JSON.stringify(body),
    })
  try {
    return await call()
  } catch (error) {
    if (error.status !== 404) throw error
    await openSession()
    return call()
  }
}

/* -------------------------------------------------------------- Word bridge */

/** Every citation control in the document, in document order. */
async function readFields(context) {
  const controls = context.document.body.contentControls
  controls.load('items/id,items/tag')
  await context.sync()
  const map = fieldMap()
  return controls.items
    .filter((c) => c.tag === TAG)
    .map((c) => ({
      control: c,
      field: {
        id: String(c.id),
        text: '',
        // A control the map has never heard of was pasted in from elsewhere.
        // Send it with no keys: the server leaves it out of the plan, so the
        // text the author can see on the page stays as it is. Dropping the
        // field from the snapshot instead would renumber the document around
        // a citation that is still visibly in it.
        citation: map[String(c.id)] || { keys: [] },
      },
    }))
}

/** Write the server's plan back into the document. */
async function applyPlan(context, entries, plan) {
  const byId = new Map(entries.map((e) => [e.field.id, e.control]))
  for (const rendered of plan.updatedFields || []) {
    const control = byId.get(rendered.id)
    if (control) control.insertHtml(rendered.text, 'Replace')
  }
  await context.sync()
}

/* --------------------------------------------------------------- operations */

async function insertCitation(citation) {
  await withBusy('Inserting…', async () => {
    let newId = null
    await Word.run(async (context) => {
      const range = context.document.getSelection()
      const control = range.insertContentControl()
      control.tag = TAG
      control.title = 'Yinkote citation'
      control.appearance = 'BoundingBox'
      control.cannotEdit = false
      control.load('id')
      await context.sync()

      newId = String(control.id)
      const map = fieldMap()
      map[newId] = citation
      settings().set(FIELDS_KEY, map)
    })
    await saveSettings()
    await rerender('Citation inserted.')
  })
}

/**
 * Re-render every citation, and the bibliography if the document has one.
 *
 * This is the only path that writes citation text. Insert, refresh and a style
 * change all end up here, because in a numeric style they are the same event:
 * the whole document's numbering may have moved.
 */
async function rerender(message) {
  await Word.run(async (context) => {
    const entries = await readFields(context)
    const plan = await session('/refresh', { fieldsSnapshot: entries.map((e) => e.field) })
    await applyPlan(context, entries, plan)
    await syncBibliography(context, plan)
  })
  if (message) say(message)
}

/** Refresh an existing bibliography in place; do nothing if there is none. */
async function syncBibliography(context, plan) {
  const controls = context.document.body.contentControls
  controls.load('items/id,items/tag')
  await context.sync()
  const bib = controls.items.find((c) => c.tag === BIB_TAG)
  if (!bib) return
  bib.insertHtml(bibliographyHtml(plan), 'Replace')
  await context.sync()
}

function bibliographyHtml(plan) {
  const entries = plan.bibliography || []
  if (!entries.length) return '<p></p>'
  return entries.map((e) => `<p>${e.text}</p>`).join('')
}

async function insertBibliography() {
  await withBusy('Building bibliography…', async () => {
    await Word.run(async (context) => {
      const entries = await readFields(context)
      // A different shape from a Plan: `/bibliography` answers with the
      // rendered list only. Named apart from `plan` on purpose — one variable
      // meaning two shapes is how `plan.updated` got written for a field that
      // is sent as `updatedFields`.
      const rendered = await session('/bibliography', {
        fieldsSnapshot: entries.map((e) => e.field),
      })

      const controls = context.document.body.contentControls
      controls.load('items/id,items/tag')
      await context.sync()

      let bib = controls.items.find((c) => c.tag === BIB_TAG)
      if (!bib) {
        const range = context.document.getSelection()
        bib = range.insertContentControl()
        bib.tag = BIB_TAG
        bib.title = 'Yinkote bibliography'
        bib.appearance = 'BoundingBox'
      }
      bib.insertHtml(bibliographyHtml({ bibliography: rendered.entries }), 'Replace')
      await context.sync()
    })
    say('Bibliography updated.')
  })
}

async function changeStyle(styleId) {
  await withBusy('Restyling…', async () => {
    const prefs = { styleId, format: 'html' }
    const result = await session('/prefs', prefs)
    state.prefs = result.prefs
    settings().set('yinkote.prefs', result.prefs)
    await saveSettings()
    // The server tells us the document is now stale rather than us assuming
    // it: a style change that did not change the rendering should not rewrite
    // every field.
    if (result.refreshRequired) await rerender('Style applied.')
    else say('Style applied.')
  })
}

/* ---------------------------------------------------------------------- UI */

function say(message, kind) {
  const node = el('status')
  node.textContent = message || ''
  if (kind) node.dataset.kind = kind
  else delete node.dataset.kind
}

async function withBusy(message, work) {
  if (state.busy) return
  state.busy = true
  setDisabled(true)
  say(message)
  try {
    await work()
  } catch (error) {
    say(describe(error), 'error')
  } finally {
    state.busy = false
    setDisabled(false)
  }
}

function describe(error) {
  if (error && error.status === 401) return 'The server wants an API key.'
  const text = (error && (error.message || String(error))) || 'Something went wrong.'
  // Word's own errors arrive with a stack attached; the pane is 300px wide.
  return text.split('\n')[0].slice(0, 160)
}

function setDisabled(disabled) {
  for (const id of ['refresh', 'insert', 'bibliography', 'style']) {
    const node = el(id)
    if (node) node.disabled = disabled
  }
}

function describeItem(item) {
  const names = (item.creators || [])
    .map((c) => c.lastName || c.name || '')
    .filter(Boolean)
  const who = names.length > 2 ? `${names[0]} et al.` : names.join(' & ')
  const year = (item.date || '').slice(0, 4)
  return [who, year, item.publicationTitle].filter(Boolean).join(' · ')
}

function renderResults() {
  const list = el('results')
  list.textContent = ''
  if (!state.results.length) {
    const empty = document.createElement('li')
    empty.className = 'empty'
    empty.textContent = 'No matches.'
    list.append(empty)
    return
  }
  for (const item of state.results) {
    const row = document.createElement('li')
    row.tabIndex = 0
    // textContent throughout: a title is library data, and library data can
    // contain angle brackets.
    const title = document.createElement('span')
    title.className = 't'
    title.textContent = item.title || '(untitled)'
    const meta = document.createElement('span')
    meta.className = 'm'
    meta.textContent = describeItem(item)
    row.append(title, meta)
    row.addEventListener('click', () => pick(item))
    row.addEventListener('keydown', (event) => {
      if (event.key === 'Enter') pick(item)
    })
    list.append(row)
  }
}

function pick(item) {
  state.picked = item
  el('pending').hidden = false
  el('pending-title').textContent = item.title || '(untitled)'
  el('locator').focus()
}

function clearPick() {
  state.picked = null
  el('pending').hidden = true
  for (const id of ['prefix', 'locator', 'suffix']) el(id).value = ''
}

let searchTimer = null
function onSearchInput() {
  clearTimeout(searchTimer)
  searchTimer = setTimeout(runSearch, 180)
}

async function runSearch() {
  const text = el('q').value.trim()
  if (!text) {
    state.results = []
    renderResults()
    return
  }
  try {
    const response = await api(
      `/libraries/${state.library}/items?q=${encodeURIComponent(text)}&limit=25`,
    )
    state.results = response.items || []
    renderResults()
  } catch (error) {
    say(describe(error), 'error')
  }
}

async function loadStyles() {
  const styles = await api('/citation-styles')
  const select = el('style')
  select.textContent = ''
  for (const style of styles) {
    const option = document.createElement('option')
    option.value = style.id
    option.textContent = style.name
    select.append(option)
  }
  select.value = state.prefs.styleId
}

function wire() {
  el('q').addEventListener('input', onSearchInput)
  el('pending-cancel').addEventListener('click', clearPick)
  el('refresh').addEventListener('click', () => withBusy('Refreshing…', () => rerender('Refreshed.')))
  el('bibliography').addEventListener('click', insertBibliography)
  el('style').addEventListener('change', (event) => changeStyle(event.target.value))
  el('insert').addEventListener('click', () => {
    if (!state.picked) return
    const citation = {
      keys: [state.picked.key],
      prefix: el('prefix').value.trim() || undefined,
      locator: el('locator').value.trim() || undefined,
      suffix: el('suffix').value.trim() || undefined,
    }
    clearPick()
    insertCitation(citation)
  })
}

/** Ask for the key, rather than reporting that one is needed and stopping. */
function askForKey(message) {
  el('boot').hidden = true
  el('app').hidden = true
  const gate = el('keygate')
  gate.hidden = false
  const problem = el('keygate-error')
  problem.hidden = !message
  problem.textContent = message || ''
  el('keygate-input').focus()
}

function saveKeyAndRetry() {
  const value = el('keygate-input').value.trim()
  if (!value) return
  try {
    window.localStorage.setItem(KEY_STORE, value)
  } catch {
    // Storage disabled in this webview: the key still works for this session,
    // since `authHeaders` reads it again on the next call. Saying nothing here
    // would be wrong only if the pane claimed it had been remembered.
  }
  el('keygate').hidden = true
  el('boot').hidden = false
  el('boot').textContent = 'Connecting…'
  boot()
}

async function boot() {
  try {
    const libraries = await api('/libraries')
    state.library = (libraries[0] && libraries[0].id) || 1
    await openSession()
    await loadStyles()
    wire()
    el('boot').hidden = true
    el('app').hidden = false
    el('q').focus()
  } catch (error) {
    // A key is the one failure the user can do something about from here.
    if (error && error.status === 401) {
      const tried = (() => {
        try {
          return Boolean(window.localStorage.getItem(KEY_STORE))
        } catch {
          return false
        }
      })()
      askForKey(tried ? 'That key was not accepted.' : '')
      return
    }
    el('boot').textContent = `Cannot reach Yinkote: ${describe(error)}`
  }
}

if (typeof document !== 'undefined' && el('keygate-save')) {
  el('keygate-save').addEventListener('click', saveKeyAndRetry)
  el('keygate-input').addEventListener('keydown', (e) => {
    if (e.key === 'Enter') saveKeyAndRetry()
  })
}

if (typeof Office !== 'undefined' && Office.onReady) {
  Office.onReady((info) => {
    if (info.host !== Office.HostType.Word) {
      el('boot').textContent = 'This pane only works in Word.'
      return
    }
    boot()
  })
}

// Exported for the pure-function tests; harmless in the browser.
if (typeof module !== 'undefined') {
  module.exports = { describeItem, bibliographyHtml, describe, boot, askForKey }
}
