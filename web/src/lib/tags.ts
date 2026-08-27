/** Tag colour.
 *
 *  A tag with no colour of its own still gets one, derived from its name. The
 *  alternative — everything grey until somebody sits down and assigns colours
 *  to two hundred tags — means nobody ever has colours, because that afternoon
 *  never comes.
 *
 *  Derived, not random: the same tag must be the same colour in the sidebar, in
 *  the table and tomorrow. A colour that changes between renders is worse than
 *  no colour, because the eye learns it and is then misled.
 *
 *  Stored as a *name* from a small palette rather than a hex value, for the
 *  same reason collections are: a named colour keeps its meaning when the theme
 *  changes and cannot produce an unreadable combination.
 */

export const TAG_COLOURS = [
  'red',
  'amber',
  'green',
  'blue',
  'violet',
  'cyan',
  'pink',
  'lime',
] as const

export type TagColour = (typeof TAG_COLOURS)[number]

/**
 * A stable hash of the name.
 *
 * FNV-1a over the code points: short, well spread for short strings, and
 * defined on every character rather than on bytes — so a CJK tag gets the same
 * treatment as an English one instead of colliding through a lossy conversion.
 */
function hash(name: string): number {
  let h = 0x811c9dc5
  for (let i = 0; i < name.length; i += 1) {
    h ^= name.codePointAt(i) ?? 0
    h = Math.imul(h, 0x01000193) >>> 0
  }
  return h
}

/**
 * The colour a tag should be drawn in.
 *
 * An explicit colour always wins; an unrecognised one is ignored rather than
 * honoured, because a library written by a newer version may name a colour this
 * build cannot draw, and a tag is not worth losing over that.
 */
export function tagColour(name: string, stored?: string | null): TagColour {
  if (stored && (TAG_COLOURS as readonly string[]).includes(stored)) {
    return stored as TagColour
  }
  return TAG_COLOURS[hash(name) % TAG_COLOURS.length]!
}

/** Whether a tag is wearing a colour somebody chose. */
export function hasChosenColour(stored?: string | null): boolean {
  return !!stored && (TAG_COLOURS as readonly string[]).includes(stored)
}
