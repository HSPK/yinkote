import { describe, expect, it } from 'vitest'

import { rankMatches, subsequenceMatch } from './fuzzy'

describe('subsequenceMatch', () => {
  it('matches an empty query against anything', () => {
    expect(subsequenceMatch('', 'whatever')).toBe(true)
  })

  it('matches scattered characters in order', () => {
    expect(subsequenceMatch('nwp', 'New Plugin')).toBe(true)
    expect(subsequenceMatch('npi', 'New Plugin')).toBe(true)
  })

  it('rejects characters that are absent', () => {
    expect(subsequenceMatch('nwq', 'New Plugin')).toBe(false)
  })

  it('rejects characters out of order', () => {
    expect(subsequenceMatch('pn', 'New Plugin')).toBe(true)
    expect(subsequenceMatch('gp', 'Plugin')).toBe(false)
  })

  it('is case-insensitive', () => {
    expect(subsequenceMatch('NEW', 'new session')).toBe(true)
  })
})

describe('rankMatches', () => {
  const items = ['重建搜索索引', '重新扫描插件', '打开：回收站', '新建条目…']
  const rank = (q: string) => rankMatches(q, items, (s) => s)

  it('returns everything for an empty query', () => {
    expect(rank('')).toEqual(items)
  })

  it('puts an exact match first', () => {
    expect(rank('新建条目…')[0]).toBe('新建条目…')
  })

  it('prefers prefix matches over substring matches', () => {
    const ordered = rankMatches('open', ['reopen file', 'open file'], (s) => s)
    expect(ordered[0]).toBe('open file')
  })

  it('prefers substring matches over scattered subsequences', () => {
    const ordered = rankMatches('abc', ['a-b-c', 'xabcx'], (s) => s)
    expect(ordered[0]).toBe('xabcx')
  })

  it('drops non-matches', () => {
    expect(rank('zzzz')).toEqual([])
  })

  it('is stable within a tier', () => {
    const ordered = rankMatches('重', items, (s) => s)
    expect(ordered).toEqual(['重建搜索索引', '重新扫描插件'])
  })

  it('works on objects via the label accessor', () => {
    const commands = [{ label: 'Reindex' }, { label: 'Reload' }]
    expect(rankMatches('reload', commands, (c) => c.label)).toEqual([{ label: 'Reload' }])
  })
})
