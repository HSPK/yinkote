/** Wire types mirroring the Rust API. Kept hand-written and small so the
 *  contract stays obvious; `/api/v1/schema` drives everything field-related. */

export interface Creator {
  creatorType: string
  firstName?: string
  lastName?: string
  name?: string
}

export interface ItemTag {
  tag: string
  type?: number
}

export type MatchSource = 'keyword' | 'fuzzy' | 'semantic' | 'tag' | 'field'

export interface SearchHit {
  key: string
  score: number
  snippet?: string
  sources: MatchSource[]
}

/** Fields are schema-driven, so anything unknown is still carried through. */
/** What a row has attached. Derived by the server from the child attachments. */
export type AttachmentKind = 'pdf' | 'snapshot' | 'link' | 'file'

export interface Item extends Record<string, unknown> {
  key: string
  libraryId: number
  itemType: string
  parentKey?: string
  creators: Creator[]
  tags: ItemTag[]
  collections: string[]
  version: number
  deleted: boolean
  dateAdded: number
  dateModified: number
  title?: string
  date?: string
  abstractNote?: string
  match?: SearchHit
  /** Absent when the row has nothing attached. */
  attachments?: AttachmentKind[]
}

/** A long job the server is running for us. */
export interface Task {
  id: string
  kind: string
  phase: 'running' | 'done' | 'failed' | 'cancelled'
  message: string
  done: number
  /** 0 when the job cannot say how much there is to do. */
  total: number
  startedAt: number
  finishedAt?: number
  /** Counters only this kind of job has, updated as it goes. */
  detail?: Record<string, unknown>
  result?: Record<string, unknown>
  error?: string
}

/** How a document was left, so it can be reopened where it was. */
export interface ReaderState {
  lastPage: number
  zoom: number
  scrollMode: 'paged' | 'continuous'
  sidebar: boolean
}

export interface Collection {
  key: string
  libraryId: number
  name: string
  parentKey?: string
  sortIndex: number
  /** Palette name, not a colour value. */
  color?: string
  /** Icon name the app resolves to a drawing it ships. */
  icon?: string
  version: number
  /** Milliseconds since the epoch; 0 when the collection predates recording. */
  dateAdded: number
  dateModified: number
  itemCount: number
}

export interface Tag {
  name: string
  color?: string
  count: number
  type: number
}

export interface Page<T> {
  items: T[]
  total: number
  /** Absent for a browse, which counts exactly. Present when the total is a
   *  floor because a search filled its candidate pool. */
  approximate?: boolean
  /** Rows are in relevance order; any requested sort was not applied. */
  ranked?: boolean
  offset: number
  limit: number
}

export interface FieldDef {
  type: string
  label: string
  labelEn: string
}

export interface ItemTypeDef {
  type: string
  label: string
  labelEn: string
  csl: string
  fields: string[]
  creatorTypes: string[]
  internal?: boolean
}

export interface Schema {
  version: number
  baseFields: string[]
  fields: Record<string, FieldDef>
  itemTypes: ItemTypeDef[]
}

export interface PluginStatus {
  id: string
  name: string
  version: string
  description?: string
  author?: string
  state: 'stopped' | 'starting' | 'ready' | 'disabled' | 'failed'
  error?: string
  capabilities: string[]
  permissions: string[]
  hooks: string[]
  contributions: {
    metadataSources: { id: string; label: string; pluginId: string }[]
    importers: { id: string; label: string; pluginId: string }[]
    exporters: { id: string; label: string; pluginId: string }[]
    itemActions: { id: string; label: string; pluginId: string }[]
    badges: BadgeDescriptor[]
  }
  calls: number
  failures: number
  avgLatencyMs: number
  source: string
}

export interface Stats {
  items: number
  trashed: number
  collections: number
  tags: number
  plugins: number
  version: number
  uptimeSecs: number
  wsClients: number
  search: { documents: number; embedded: number; dimensions: number; provider: string }
}

export type SearchMode = 'hybrid' | 'keyword' | 'fuzzy' | 'semantic'

export interface ListQuery {
  q?: string
  mode?: SearchMode
  collection?: string
  tag?: string[]
  itemType?: string[]
  trash?: 'exclude' | 'only' | 'include'
  /** Papers only, excluding their attachments, notes and highlights. */
  topLevel?: boolean
  sort?: string
  direction?: 'asc' | 'desc'
  limit?: number
  offset?: number
}

export interface DetectedIdentifier {
  kind: string
  value: string
}

/** A draft resolved from an identifier, not yet written to the library. */
export interface Resolution {
  kind: string
  identifier: string
  source: string
  draft: Record<string, unknown> & { itemType: string; title?: string }
}

/** Why an identifier produced nothing. Wording lives in the catalogues. */
export type ScrapeProblem = 'notFound' | 'blocked' | 'unavailable'

export interface Unresolved {
  kind: string
  identifier: string
  problem: ScrapeProblem
  /** The source's own words: English, untranslated. Tooltip only. */
  detail: string
}

export interface ResolveResponse {
  identifiers: DetectedIdentifier[]
  resolutions: Resolution[]
  unresolved: Unresolved[]
  tookMs: number
}

export interface QuickAddResponse {
  created: Item[]
  duplicates: { identifier: string; existingKey: string; title: string }[]
  unresolved: Unresolved[]
  version: number
}

export interface SourceInfo {
  id: string
  label: string
  supports: string[]
}

export type AccessState =
  | { state: 'private' }
  | { state: 'protected' }
  | { state: 'open' }

export type ConnectorStatus =
  | { state: 'off' }
  | { state: 'listening'; port: number }
  | { state: 'unavailable'; port: number }

export interface ServerInfo {
  ok: boolean
  service: string
  version: string
  apiVersion: number
  pluginApiVersion: number
  uptimeSecs: number
  defaultLibrary: number
  dataDir: string
  pluginDirs: string[]
  bind: string
  /** What browser saving is doing — asked for is not the same as working. */
  connector: ConnectorStatus
  /** Who can reach this library. */
  access: AccessState
}

export interface SmartCollection {
  key: string
  libraryId: number
  name: string
  color?: string
  icon?: string
  /** Exactly what would be typed into the search box. */
  query: string
  mode: SearchMode
  sort: string
  direction: 'asc' | 'desc'
  sortIndex: number
  version: number
  dateAdded: number
  dateModified: number
  /** Present when `itemCount` is a floor: a text query is counted by
   *  running it, and a ranked search scores a bounded pool. */
  itemCountApproximate?: boolean
  itemCount?: number
}

export interface Conversation {
  key: string
  libraryId: number
  title: string
  scope?: string
  messageCount: number
  createdAt: number
  updatedAt: number
}

export interface Message {
  id: number
  role: 'user' | 'assistant' | 'tool' | 'system'
  content: string
  meta?: Record<string, unknown>
  /** Item keys the message named with `@`. */
  mentions?: string[]
  createdAt: number
}

export interface MessagePage {
  messages: Message[]
  /** Whether anything older exists. */
  hasMore: boolean
}

export interface BadgeDescriptor {
  id: string
  label: string
  description?: string
  needs: string[]
  width?: number
  /** Orderable, which requires the plugin to return a `rank` with each value. */
  sortable?: boolean
  pluginId: string
}

export interface BadgeValue {
  badge: string
  text: string
  /** Higher sorts first when descending; the plugin decides what higher means. */
  rank?: number
  /** A severity or a palette colour name, so each level can differ. */
  tone?: string
  title?: string
  pluginId: string
}

export interface AgentStatus {
  configured: boolean
  model?: string
  endpoint?: string
  /** Whether a key is stored. The key itself is never sent back. */
  hasApiKey?: boolean
  allowCommands?: boolean
  maxSteps?: number
  tools?: string[]
  writes?: string[]
  /** Every tool that could exist, so one that is off can be switched back on. */
  allTools?: string[]
  disabledTools?: string[]
  skills?: { name: string; description: string; enabled: boolean }[]
}

export interface ImportPreview {
  items: number
  collections: number
  tags: number
  attachments: number
  notes: number
  annotations: number
}

export interface ImportResult {
  items: number
  /** Already present, and brought up to date. */
  updated: number
  collections: number
  /** Attachment files copied out of Zotero's storage. */
  files: number
  /** The user's own notes, brought across. */
  notes: number
  /** Highlights and margin notes made inside PDFs. */
  annotations: number
  failed: number
  total: number
}

/** A citation style the server can render. */
export interface CitationStyle {
  id: string
  name: string
  /** Numeric styles cite `[1]`; the rest cite `(Author, year)`. */
  numeric: boolean
}

export interface CitationRender {
  style: string
  /** The marker for running text, one per key, in the order asked for. */
  citations: string[]
  /** The bibliography entry, one per key, in the order asked for. */
  bibliography: string[]
}

/** A node in the relationship graph: one item, however many edges reach it. */
export interface GraphNode {
  key: string
  title: string
  year?: number | null
  itemType: string
  /** The item the neighbourhood is about. Exactly one node has this. */
  focus?: boolean
  /** A cited work this library does not hold. It cannot be opened. */
  external?: boolean
}

/** Why two items are connected, and how strongly. */
export interface GraphEdge {
  source: string
  target: string
  relation: 'tag' | 'author' | 'collection' | 'similar' | 'cites'
  /** Shared tags or collections for structural edges; a cosine for similarity. */
  weight: number
}

export interface GraphNeighbourhood {
  focus: string
  nodes: GraphNode[]
  edges: GraphEdge[]
}

/** One work cited by another. */
export interface Citation {
  position: number
  /** The item in this library, when it holds the cited work. */
  key: string | null
  label: string
  year: number | null
  fingerprint: string
}

export interface CitationList {
  cites: Citation[]
  citedBy: Citation[]
  /** How many of `cites` are papers the library actually holds. */
  resolved: number
}

/** A work the library keeps citing and does not hold. */
export interface MissingWork {
  fingerprint: string
  /** The DOI as deposited; the fingerprint cannot be turned back into one. */
  doi: string
  label: string
  year: number | null
  /** How many papers in the library cite it. */
  citedBy: number
}

/** A run that fetches reference lists for papers that have none yet. */
export interface Harvest {
  running: boolean
  total: number
  done: number
  stored: number
  /** Papers whose publisher deposited no reference list at all. */
  empty: number
  failed: number
  stopped: boolean
  message: string | null
}

/** One entry in an agent turn, in the order it happened. */
export type RunStep =
  | { kind: 'text'; content: string }
  | { kind: 'thinking'; content: string }
  | {
      kind: 'tool'
      name: string
      arguments: unknown
      result: string
      writes: boolean
      /** The stored result was cut: a tool's whole answer is not kept for ever
       *  in a conversation that is reloaded every time it is opened. */
      clipped?: boolean
    }

/** What a conversation's turn is doing. */
export interface RunState {
  /** Epoch milliseconds when the turn started. */
  startedAt?: number
  running: boolean
  question: string
  steps: RunStep[]
  /** Empty while the turn is going. */
  reply: string
  truncated: boolean
  /** True when the user stopped it, as opposed to the loop running out. */
  stopped: boolean
  error: string | null
  /** What kind of failure, as a key the catalogues name. The message itself
   *  is written in English and often carries the upstream service's JSON. */
  errorProblem?: string
  /** The answer as it arrives, before it lands as a step. */
  partial?: string
  partialReasoning?: string
}

/** One file the library is waiting for. */
export interface Download {
  id: number
  itemKey: string
  url: string
  state: 'waiting' | 'running' | 'done' | 'failed'
  attempts: number
  /** Why it failed. Kept beside the row because it is what a retry is decided from. */
  error: string
  title: string
  bytes: number
  updatedAt: number
}

/** One stored file, with the item it belongs to. */
export interface LibraryFile {
  key: string
  parentKey: string | null
  parentTitle: string
  filename: string
  contentType: string
  /** Where it was fetched from. */
  url: string
  /** From disk, not from the record — a file the database believes in and the
   *  disk does not is exactly what this view is for finding. */
  bytes: number
}

export interface RenamePlan {
  template: string
  /** How many files would change. */
  total: number
  /** A sample of them — enough to judge the pattern, not all of them. */
  changes: { key: string; from: string; to: string }[]
}
