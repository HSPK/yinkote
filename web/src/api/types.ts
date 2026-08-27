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

export interface ResolveResponse {
  identifiers: DetectedIdentifier[]
  resolutions: Resolution[]
  tookMs: number
}

export interface QuickAddResponse {
  created: Item[]
  duplicates: { identifier: string; existingKey: string; title: string }[]
  unresolved: DetectedIdentifier[]
  version: number
}

export interface SourceInfo {
  id: string
  label: string
  supports: string[]
}

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
  createdAt: number
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
