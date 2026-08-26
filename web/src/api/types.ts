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
  pluginId: string
}

export interface BadgeValue {
  badge: string
  text: string
  tone?: 'high' | 'mid' | 'low' | 'neutral'
  title?: string
  pluginId: string
}
