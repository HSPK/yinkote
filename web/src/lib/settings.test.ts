import { describe, expect, it } from 'vitest'

import { filterSettings, type SettingSection } from './settings'

const field = (id: string, label: string, extra: Partial<SettingSection['fields'][0]> = {}) => ({
  id,
  label,
  render: () => null,
  ...extra,
})

const SECTIONS: SettingSection[] = [
  {
    id: 'appearance',
    title: 'Appearance',
    fields: [
      field('language', 'Language'),
      field('theme', 'Theme', { keywords: 'colour dark light' }),
    ],
  },
  {
    id: 'storage',
    title: 'Storage',
    fields: [field('dataDir', 'Data directory', { hint: 'Where the library lives' })],
  },
]

describe('filterSettings', () => {
  it('returns everything for an empty query', () => {
    expect(filterSettings(SECTIONS, '  ')).toBe(SECTIONS)
  })

  it('matches a field label', () => {
    const got = filterSettings(SECTIONS, 'theme')
    expect(got.map((s) => s.id)).toEqual(['appearance'])
    expect(got[0]?.fields.map((f) => f.id)).toEqual(['theme'])
  })

  it('matches on hints and keywords the label never mentions', () => {
    expect(filterSettings(SECTIONS, 'dark')[0]?.fields[0]?.id).toBe('theme')
    expect(filterSettings(SECTIONS, 'library lives')[0]?.id).toBe('storage')
  })

  it('keeps a whole section when its title matches', () => {
    // Having asked for the section, seeing it gutted to one row is worse than
    // useless.
    expect(filterSettings(SECTIONS, 'appearance')[0]?.fields).toHaveLength(2)
  })

  it('narrows as words are added', () => {
    expect(filterSettings(SECTIONS, 'appearance language')[0]?.fields.map((f) => f.id)).toEqual([
      'language',
    ])
  })

  it('drops sections with nothing left', () => {
    expect(filterSettings(SECTIONS, 'nonsense')).toEqual([])
  })

  it('ignores case', () => {
    expect(filterSettings(SECTIONS, 'LANGUAGE')[0]?.fields[0]?.id).toBe('language')
  })
})
