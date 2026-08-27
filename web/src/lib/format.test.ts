import { describe, expect, it } from 'vitest'

import type { Item } from '../api/types'
import { compact, creatorName, creatorSummary, shortDate, snippetParts, year } from './format'

const item = (patch: Partial<Item>): Item =>
  ({
    key: 'A1B2C3D4',
    libraryId: 1,
    itemType: 'journalArticle',
    creators: [],
    tags: [],
    collections: [],
    version: 1,
    deleted: false,
    dateAdded: 0,
    dateModified: 0,
    ...patch,
  }) as Item

describe('creatorName', () => {
  it('joins two-field names', () => {
    expect(creatorName({ creatorType: 'author', firstName: 'Ada', lastName: 'Lovelace' })).toBe(
      'Ada Lovelace',
    )
  })

  it('uses the single-field name when present', () => {
    expect(creatorName({ creatorType: 'author', name: '张伟' })).toBe('张伟')
  })

  it('tolerates a missing half', () => {
    expect(creatorName({ creatorType: 'author', lastName: 'Ho' })).toBe('Ho')
  })
})

describe('creatorSummary', () => {
  const withCreators = (...names: string[]) =>
    item({ creators: names.map((lastName) => ({ creatorType: 'author', lastName })) })

  it('is empty without creators', () => {
    expect(creatorSummary(item({}))).toBe('')
  })

  it('names one author outright', () => {
    expect(creatorSummary(withCreators('Vaswani'))).toBe('Vaswani')
  })

  it('joins exactly two with an ampersand', () => {
    expect(creatorSummary(withCreators('Vaswani', 'Shazeer'))).toBe('Vaswani & Shazeer')
  })

  it('abbreviates three or more', () => {
    expect(creatorSummary(withCreators('Vaswani', 'Shazeer', 'Parmar'))).toBe('Vaswani et al.')
  })

  it('falls back to the single-field name', () => {
    expect(creatorSummary(item({ creators: [{ creatorType: 'author', name: '张伟' }] }))).toBe('张伟')
  })
})

describe('year', () => {
  it.each([
    ['2017-06-12', '2017'],
    ['June 2020', '2020'],
    ['2023', '2023'],
    ['n.d.', ''],
    ['', ''],
  ])('extracts %s -> %s', (date, expected) => {
    expect(year(item({ date }))).toBe(expected)
  })
})

describe('shortDate', () => {
  it('renders nothing for a missing timestamp', () => {
    expect(shortDate(0)).toBe('')
  })

  it('includes the year for older dates', () => {
    // 2001-09-09T01:46:40Z — safely not the current year.
    expect(shortDate(1_000_000_000_000)).toMatch(/^2001-09-0[89]$/)
  })

  it('drops the year for dates in the current one', () => {
    const now = Date.now()
    expect(shortDate(now)).not.toMatch(/^\d{4}-/)
  })
})

describe('snippetParts', () => {
  it('splits marked regions from plain text', () => {
    expect(snippetParts('a <mark>b</mark> c')).toEqual([
      { text: 'a ', mark: false },
      { text: 'b', mark: true },
      { text: ' c', mark: false },
    ])
  })

  it('handles multiple marks', () => {
    expect(snippetParts('<mark>x</mark>-<mark>y</mark>').filter((p) => p.mark)).toHaveLength(2)
  })

  it('passes unmarked text straight through', () => {
    expect(snippetParts('plain')).toEqual([{ text: 'plain', mark: false }])
  })

  it('never yields raw HTML, so React can render it safely', () => {
    const parts = snippetParts('<mark>a</mark>')
    expect(parts.every((p) => !p.text.includes('<'))).toBe(true)
  })
})

describe('compact', () => {
  it('says nothing rather than NaN when the number never arrived', () => {
    // `(undefined / 1_000_000).toFixed(1)` is "NaN", so the counts beside
    // every sidebar label would have read "NaNM" on a slow first load.
    expect(compact(undefined as unknown as number)).toBe('—')
    expect(compact(NaN)).toBe('—')
  })

  it.each([
    [0, '0'],
    [999, '999'],
    [1500, '1.5k'],
    [42_000, '42k'],
    [1_500_000, '1.5M'],
  ])('formats %i as %s', (n, expected) => {
    expect(compact(n)).toBe(expected)
  })
})
