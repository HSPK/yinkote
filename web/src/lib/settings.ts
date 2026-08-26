/** Settings described as data, so they can be searched.
 *
 *  A settings page grows until nobody can find anything in it. Describing the
 *  sections and fields as a list — rather than as JSX that happens to be
 *  arranged in sections — means filtering is a pure function over that list,
 *  section emptiness is arithmetic instead of a DOM question, and a new setting
 *  is searchable the moment it is added.
 */
import type { ReactNode } from 'react'

export interface SettingField {
  id: string
  label: string
  hint?: string
  /** Extra words the user might search for that do not appear in the label. */
  keywords?: string
  render: () => ReactNode
}

export interface SettingSection {
  id: string
  title: string
  fields: SettingField[]
}

/** Everything about a field that a search should look at. */
function haystack(field: SettingField, sectionTitle: string): string {
  return [field.label, field.hint, field.keywords, sectionTitle].filter(Boolean).join(' ')
}

/**
 * Keep only the sections and fields matching every word of the query.
 *
 * Words are ANDed so that adding a word narrows, which is what typing more
 * feels like it should do. A section whose *title* matches keeps all of its
 * fields: having searched for "appearance", seeing that section gutted to one
 * row would be worse than useless.
 */
export function filterSettings(sections: SettingSection[], query: string): SettingSection[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  if (!words.length) return sections

  const matches = (text: string) => {
    const lower = text.toLowerCase()
    return words.every((w) => lower.includes(w))
  }

  return sections
    .map((section) => {
      if (matches(section.title)) return section
      const fields = section.fields.filter((f) => matches(haystack(f, section.title)))
      return { ...section, fields }
    })
    .filter((section) => section.fields.length > 0)
}
