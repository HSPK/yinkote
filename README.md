# Yinkote

**A local-first reference manager you run yourself, and use in a browser.**

[中文](README.zh-CN.md) · [Documentation](docs/) · AGPL-3.0-or-later

Yinkote is one binary. You run it on your own machine, open a browser, and your
library is there. Nothing is uploaded, no account is created, and the program
keeps working when the network does not.

It is a working alternative to Zotero for people who would rather their
literature lived in a folder they can see, in formats they can read, on a
machine they control.

```
┌─ your machine ────────────────────────────────┐
│  yinkote  ──►  SQLite + your PDFs on disk     │
│     ▲                                          │
│     │  HTTP + WebSocket, on 127.0.0.1          │
│     ├── the workbench, in your browser         │
│     ├── the Word add-in                        │
│     └── the browser extension                  │
└────────────────────────────────────────────────┘
```

> **Status: v0.1, early.** The library format is stable and the tests are
> thorough — 903 backend tests, 627 in the workbench, and a 281-check smoke run
> against a live server — but this has not yet been used by many people on many
> machines. Keep a backup, as you would with anything holding years of reading.

---

## Install

Download the binary for your platform, make it executable, and run it.

```bash
# macOS / Linux
chmod +x yinkote
./yinkote
```

```powershell
# Windows
.\yinkote.exe
```

Then open **<http://127.0.0.1:23130>**.

That is the whole installation. There is no installer, no runtime to install
first, and no configuration file to write. The first run creates a data
directory and starts serving.

| Platform | File |
| --- | --- |
| Linux (x86-64) | `yinkote-x86_64-unknown-linux-gnu` |
| Linux (ARM64) | `yinkote-aarch64-unknown-linux-gnu` |
| macOS (Apple silicon) | `yinkote-aarch64-apple-darwin` |
| macOS (Intel) | `yinkote-x86_64-apple-darwin` |
| Windows (x86-64) | `yinkote-x86_64-pc-windows-msvc.exe` |

**Why one file.** The workbench is compiled into the binary, SQLite is
statically linked, and the only dynamic dependencies are the system C runtime.
A 20 MB download is the entire program.

### Keep it running

```bash
yinkote service install      # start automatically when you log in
yinkote service status
yinkote service uninstall
```

This writes a systemd **user** unit, a launchd agent, or a Startup-folder
script, depending on the platform. Never a system service: a personal library
does not belong to root.

### Open the workbench later

```bash
yinkote open
```

Finds the address of the server already running for this data directory, from
the directory's own lock file, and points a browser at it. It does not start a
second one.

---

## Your data

Everything lives in one directory, which you can copy, back up, or put in a
synced folder.

```
<data-dir>/
├─ yinkote.db          the library: items, notes, tags, collections, index
├─ storage/            attachments, one folder per item
├─ plugins/            plugins you have installed
└─ config.toml         written only when you change something
```

Defaults, unless `--data-dir` says otherwise:

| Platform | Location |
| --- | --- |
| Linux | `$XDG_DATA_HOME/yinkote`, or `~/.local/share/yinkote` |
| macOS | `~/Library/Application Support/Yinkote` |
| Windows | `%APPDATA%\Yinkote` |

The database is plain SQLite and the attachments are ordinary files in ordinary
folders. If Yinkote disappeared tomorrow you would still have your library.

### Coming from Zotero

Export your Zotero library (**File → Export Library**, choose *Zotero RDF* with
files, or point Yinkote at the Zotero data directory) and import it from
**Settings → Import**. Items, collections, tags, notes, annotations and
attachments come across, and duplicates are merged rather than doubled.

---

## Using it from elsewhere

By default Yinkote listens on `127.0.0.1` only — nothing outside your machine
can reach it, and no password is needed because nothing else can ask.

To reach it from a phone or another computer:

```bash
yinkote --host 0.0.0.0
```

The first time you do this, Yinkote **refuses to start without an API key**, and
tells you so — with the fix in the message. Binding beyond loopback turns off
the browser protections that made a keyless local server safe, so the key is
not optional:

```bash
YK_API_KEY="a long random string" yinkote --host 0.0.0.0
```

To keep it, put it in `config.toml` in the data directory instead:

```toml
api_key = "a long random string"
```

Then send `Authorization: Bearer <key>` with API requests; the workbench asks
for it once and remembers.

`--allow-anonymous` exists and is a bad idea: it exposes the whole library —
including deleting items and reading files — to anyone who can reach the port.
For real remote access, put Yinkote behind Tailscale, a reverse proxy with TLS,
or an SSH tunnel.

### Browser extension

```bash
yinkote --connector-port 23119
```

Yinkote then answers on the port the Zotero connector expects, so the Zotero
browser extension saves into Yinkote instead. Off unless you ask: that port
belongs to Zotero, and taking it would break a running copy.

### Word / WPS

The add-in is served by the running server. **Settings → Word add-in** shows the
manifest path and the sideload instructions for your platform.

---

## What it does

**Items and organisation.** Seventeen item types from a schema rather than
hard-coded forms; nested collections; smart collections that are saved searches;
tags with colours; a trash that really keeps things until you empty it.

**Search that finds things.** Four strategies fused into one ranking: keyword
(BM25), fuzzy (trigram + edit distance, for when you mistype), semantic
(vector), and field filters. The query language is what you would guess —
`tag:survey type:journalArticle author:hinton year:2020..2024 -tag:archived
"exact phrase"` — and Chinese is searchable without configuration.

**Reading.** A PDF reader with highlights, notes and an outline, rendered at
device resolution. Markdown notes on any paper. Annotations gathered into a note
in one gesture.

**References.** A paper's bibliography from Crossref, from Semantic Scholar for
preprints, or read off the PDF's own pages when nobody deposited one — and the
answer says which, because those are not equally reliable. What your library
cites but does not hold is a page of its own.

**Bringing papers in.** Paste a DOI, arXiv link, PubMed ID, ISBN or a plain URL;
Yinkote works out what it is, fetches the metadata, and queues the PDF. Filing a
paper into a collection fetches its file too.

**AI, if you want it.** Summaries, close readings, and a library-wide assistant
that can search, file and tag — all against an endpoint **you** configure. There
is no built-in cloud service and nothing is sent anywhere by default. Point it
at a local Ollama or llama.cpp and it never leaves the machine.

**Plugins.** Separate processes speaking JSON-RPC, with declared capabilities
and no privileged access to the database. Three ship as examples, including
journal metrics (impact factor, JCR, CAS).

**Citations.** CSL styles, a bibliography from any selection, and live citation
fields in Word.

---

## Speed

Measured on this machine against a **99,898-item** library, release build, all
of it embedded:

| Operation | p50 | p95 |
| --- | --- | --- |
| Keyword search (2 terms) | 12.8 ms | 14.5 ms |
| Keyword search (1 term) | 26.1 ms | 46.0 ms |
| Chinese keyword search | 33.0 ms | 37.1 ms |
| Fuzzy search (mistyped) | 5.7 ms | 6.5 ms |
| Semantic search | 6.7 ms | 8.3 ms |
| Hybrid search (all four, fused) | 13.9 ms | 16.0 ms |
| Hybrid + fetching the rows to show | 34.8 ms | 41.0 ms |
| Open a collection | 3.0 ms | 3.4 ms |
| File browser page | 6.7 ms | 7.6 ms |
| Create one item | 3.3 ms | 3.9 ms |

`node scripts/bench.mjs` reproduces this, seeding the corpus if the library does
not have one. **It will seed 100,000 items into whatever library you point it
at**, so give it a scratch data directory.

---

## Options

```
yinkote [OPTIONS]
yinkote open
yinkote service install|uninstall|status
```

| Option | Meaning |
| --- | --- |
| `-p, --port <PORT>` | Port to listen on (default `23130`) |
| `--host <HOST>` | Address to bind (default `127.0.0.1`) |
| `--data-dir <DIR>` | Where the library lives |
| `--web-dir <DIR>` | Serve the workbench from disk instead of the built-in copy |
| `--plugin-dir <DIR>` | An extra plugin directory; may be repeated |
| `--connector-port <PORT>` | Also answer the Zotero browser extension |
| `--allow-anonymous` | Serve a public address with no key. Read the warning above. |

Environment: `YK_DATA_DIR` `YK_HOST` `YK_PORT` `YK_WEB_DIR` `YK_API_KEY`
`YK_LOG`, plus `YK_EMBED_*` for the embedding provider and `YK_AGENT_*` for the
assistant.

---

## Building from source

You need Rust 1.85+ and Node 20+. The frontend is built first because the
binary embeds it.

```bash
(cd web && npm install && npm run build)
cargo build --release -p yk-server
./target/release/yinkote
```

A build with no `web/dist` still compiles and still runs; it serves a single
page explaining that the workbench was not built, which is a better failure than
a blank screen.

### Working on it

```bash
cargo run -p yk-server -- --data-dir ./.dev-data     # backend
(cd web && npm run dev)                              # frontend on :5273, proxying /api

cargo test --workspace                               # 903 tests
cargo clippy --workspace --all-targets -- -D warnings
(cd web && npm test)                                 # 627 tests
bash scripts/smoke.sh                                # 281 checks against a running server
node scripts/bench.mjs                               # the numbers above
```

`docs/15-development-philosophy.md` and `docs/16-workspace-rules.md` are worth
reading before changing anything: the second is a long list of mistakes already
made here and what they cost, which is the most useful thing in the repository.

---

## How it is put together

```
crates/
├─ yk-core      domain model, ports (traits), errors, events, item schema
├─ yk-store     SQLite: repositories, migrations, FTS/trigram/vector upkeep
├─ yk-search    hybrid retrieval: BM25 + fuzzy + vector, fused
├─ yk-pdf       text extraction, reference parsing
├─ yk-scrape    identifier resolution, metadata sources, external search
├─ yk-cite      CSL citation and bibliography rendering
├─ yk-ai        embedding and chat provider abstractions
├─ yk-agent     the assistant: tools, turns, skills
├─ yk-import    Zotero and bibliography import
├─ yk-plugin    plugin runtime: discovery, JSON-RPC, hooks, lifecycle
└─ yk-server    HTTP/WebSocket, background workers, the embedded workbench
web/            React + TypeScript workbench
plugins/        example plugins
```

Dependencies point inward: `yk-server → yk-{store,search,plugin} → yk-core`.
Everything crossing a layer goes through a trait in `yk-core::ports`, so the
search engine, the plugin runtime and the embedding provider can each be
replaced without touching their callers.

A few decisions that carry their weight:

- **Derived data is maintained in the same transaction as the write.** The
  full-text index, the trigram index and the embedding queue cannot drift from
  the items, because there is no path that updates one without the others.
- **`BEGIN IMMEDIATE` for writes.** A deferred transaction cannot upgrade its
  lock under concurrency, and SQLite returns `SQLITE_BUSY` immediately rather
  than waiting out the busy timeout.
- **Background workers stand aside.** Anything that takes the database
  exclusively — checkpoints, index compaction — waits for writing to stop, so a
  bulk import never fights the housekeeping.
- **Plugins have no privileges.** They reach the library through the same
  permissioned API a third-party client would.

---

## Documentation

| | |
| --- | --- |
| [`docs/00-overview.md`](docs/00-overview.md) | What this is and who it is for |
| [`docs/01-architecture.md`](docs/01-architecture.md) | Layers, crates, boundaries |
| [`docs/03-data-model.md`](docs/03-data-model.md) | Items, collections, relations |
| [`docs/04-api-design.md`](docs/04-api-design.md) | The HTTP API |
| [`docs/06-search-and-pdf.md`](docs/06-search-and-pdf.md) | Retrieval and PDF handling |
| [`docs/08-security-and-deploy.md`](docs/08-security-and-deploy.md) | Threat model, packaging |
| [`docs/11-agents.md`](docs/11-agents.md) | The assistant and its limits |
| [`docs/14-storage-layout.md`](docs/14-storage-layout.md) | What is on disk, and where |
| [`docs/16-workspace-rules.md`](docs/16-workspace-rules.md) | Every mistake made here so far |

---

## Licence

AGPL-3.0-or-later. See [LICENSE](LICENSE).

In short: use it for anything, including commercially; if you modify it and let
other people use it over a network, publish your changes. A reference manager
holding a decade of somebody's reading should not be something they can be
locked out of.
