/** Collection appearance: a small palette and a small icon set.
 *
 *  Both are stored as *names*, not as colours or drawings. A named colour keeps
 *  its meaning when the theme changes and cannot produce an unreadable
 *  combination, and a named icon resolves to something the app already ships,
 *  so a library file never depends on assets it does not contain.
 */
import { Icon, type IconName } from '../ui'

export const COLLECTION_COLOURS = ['red', 'amber', 'green', 'blue', 'violet'] as const

export type CollectionColour = (typeof COLLECTION_COLOURS)[number]

/** Icons a collection may wear, keyed by the name stored in the database. */
export const COLLECTION_ICONS = [
  'folder',
  'book',
  'smart',
  'tag',
  'star',
  'flask',
] as const

export type CollectionIcon = (typeof COLLECTION_ICONS)[number]

const ICON_COMPONENTS: Record<CollectionIcon, IconName> = {
  folder: 'Folder',
  book: 'Library',
  smart: 'Smart',
  tag: 'Tag',
  star: 'Star',
  flask: 'Flask',
}

/**
 * The icon component for a stored name.
 *
 * Falls back rather than throwing: a library edited by a newer version may name
 * an icon this build has never heard of, and a missing drawing is not a reason
 * to lose the collection.
 */
export function collectionIcon(name: string | undefined, fallback: IconName = 'Folder') {
  const key = name as CollectionIcon | undefined
  return Icon[(key && ICON_COMPONENTS[key]) || fallback]
}

/** A stored colour, or `undefined` when unset or unrecognised. */
export function collectionColour(name: string | undefined): CollectionColour | undefined {
  return COLLECTION_COLOURS.includes(name as CollectionColour)
    ? (name as CollectionColour)
    : undefined
}
