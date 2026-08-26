import { describe, expect, it } from 'vitest'

import {
  addToken,
  completeTag,
  inferMode,
  modeReason,
  OPERATORS,
  parseQuery,
  pendingTag,
  queryFromRules,
  rulesFromQuery,
  removeToken,
  serialiseQuery,
  tokenSource,
} from './query'

describe('parseQuery', () => {
  it('separates operators from free text', () => {
    const got = parseQuery('diffusion tag:survey type:book')
    expect(got.map((t) => [t.field, t.value])).toEqual([
      ['text', 'diffusion'],
      ['tag', 'survey'],
      ['type', 'book'],
    ])
  })

  it('understands negation on tags', () => {
    const [token] = parseQuery('-tag:obsolete')
    expect(token).toMatchObject({ field: 'tag', value: 'obsolete', negated: true })
  })

  it('accepts the Chinese operator names the server accepts', () => {
    expect(parseQuery('标签:综述')[0]).toMatchObject({ field: 'tag', value: '综述' })
    expect(parseQuery('作者:zhang')[0]).toMatchObject({ field: 'author', value: 'zhang' })
  })

  it('treats creator as an alias of author, since the server does', () => {
    expect(parseQuery('creator:zhang')[0]?.field).toBe('author')
  })

  it('keeps a quoted phrase in one piece', () => {
    const [token] = parseQuery('"attention is all you need"')
    expect(token).toMatchObject({ field: 'text', value: 'attention is all you need', phrase: true })
  })

  it('leaves an unknown operator as text rather than dropping it', () => {
    // Silently discarding "doi:10.1000/x" would be worse than searching for it.
    expect(parseQuery('doi:10.1000/x')[0]).toMatchObject({ field: 'text', value: 'doi:10.1000/x' })
  })

  it('leaves a bare colon alone', () => {
    expect(parseQuery(':x')[0]?.field).toBe('text')
    expect(parseQuery('tag:')[0]?.field).toBe('text')
  })

  it('round-trips through serialisation', () => {
    const input = 'diffusion tag:survey -tag:old type:book year:2020..2024'
    expect(serialiseQuery(parseQuery(input))).toBe(input)
  })
})

describe('editing a query', () => {
  it('removes one chip and leaves the rest untouched', () => {
    expect(removeToken('diffusion tag:survey type:book', 1)).toBe('diffusion type:book')
  })

  it('adds a token', () => {
    expect(addToken('diffusion', { field: 'tag', value: 'survey', source: '' })).toBe(
      'diffusion tag:survey',
    )
  })

  it('does not add the same constraint twice', () => {
    const once = addToken('', { field: 'tag', value: 'survey', source: '' })
    expect(addToken(once, { field: 'tag', value: 'survey', source: '' })).toBe(once)
  })

  it('quotes a value containing a space so it survives the round trip', () => {
    const source = tokenSource({ field: 'author', value: 'Wei Zhang', source: '' })
    expect(source).toBe('author:"Wei Zhang"')
    expect(parseQuery(source)[0]).toMatchObject({ field: 'author', value: 'Wei Zhang' })
  })
})

describe('inferMode', () => {
  it('ranks ordinary text with everything it has', () => {
    expect(inferMode('diffusion models for molecules')).toBe('hybrid')
    expect(modeReason('diffusion models')).toBe('text')
  })

  it('takes a quoted phrase literally', () => {
    // Semantic neighbours of an exact phrase are not what was asked for.
    expect(inferMode('"attention is all you need"')).toBe('keyword')
    expect(modeReason('"attention is all you need"')).toBe('phrase')
  })

  it('treats a filter-only query as a lookup', () => {
    expect(inferMode('tag:survey type:book')).toBe('keyword')
    expect(inferMode('')).toBe('keyword')
    expect(modeReason('tag:survey')).toBe('filter')
  })

  it('does not embed one or two latin characters', () => {
    expect(inferMode('ab')).toBe('keyword')
    expect(modeReason('ab')).toBe('short')
  })

  it('treats two CJK characters as a real word', () => {
    expect(inferMode('综述')).toBe('hybrid')
  })
})

describe('tag completion', () => {
  it('sees a half-typed tag operator', () => {
    expect(pendingTag('diffusion tag:sur')).toBe('sur')
    expect(pendingTag('tag:')).toBe('')
    expect(pendingTag('-tag:obs')).toBe('obs')
  })

  it('ignores a completed operator or plain text', () => {
    expect(pendingTag('tag:survey ')).toBeNull()
    expect(pendingTag('diffusion')).toBeNull()
  })

  it('recognises the Chinese alias too, without repeating it', () => {
    expect(pendingTag('标签:综')).toBe('综')
  })

  it('replaces the partial operator instead of appending beside it', () => {
    expect(completeTag('diffusion tag:sur', 'survey')).toBe('diffusion tag:survey')
    expect(completeTag('-tag:obs', 'obsolete')).toBe('-tag:obsolete')
  })

  it('quotes a completed tag that contains a space', () => {
    expect(completeTag('tag:mach', 'machine learning')).toBe('tag:"machine learning"')
  })
})

describe('rules', () => {
  const roundTrip = (q: string) => queryFromRules(rulesFromQuery(q))

  it('reads tags as is / is not', () => {
    expect(rulesFromQuery('tag:survey -tag:old')).toEqual([
      { field: 'tag', op: 'is', value: 'survey' },
      { field: 'tag', op: 'isNot', value: 'old' },
    ])
  })

  it('reads every year form the server accepts', () => {
    expect(rulesFromQuery('year:2020')[0]).toMatchObject({ op: 'is', value: '2020' })
    expect(rulesFromQuery('year:>=2020')[0]).toMatchObject({ op: 'from', value: '2020' })
    expect(rulesFromQuery('year:<=2024')[0]).toMatchObject({ op: 'to', value: '2024' })
    expect(rulesFromQuery('year:2020..2024')[0]).toMatchObject({
      op: 'between',
      value: '2020',
      value2: '2024',
    })
  })

  it('distinguishes a phrase from loose text', () => {
    expect(rulesFromQuery('"exact words"')[0]).toMatchObject({ op: 'phrase' })
    expect(rulesFromQuery('loose')[0]).toMatchObject({ op: 'contains' })
  })

  it('survives a round trip through the editor', () => {
    for (const q of [
      'tag:survey',
      '-tag:old type:book',
      'year:2020..2024',
      'year:>=2020',
      'diffusion tag:survey author:zhang',
      '"exact words" tag:survey',
    ]) {
      expect(roundTrip(q), q).toBe(q)
    }
  })

  it('drops an empty rule instead of emitting a broken operator', () => {
    expect(queryFromRules([{ field: 'tag', op: 'is', value: '  ' }])).toBe('')
  })

  it('falls back to a plain year when a range is half-filled', () => {
    expect(queryFromRules([{ field: 'year', op: 'between', value: '2020' }])).toBe('year:2020')
  })

  it('offers only operators that mean something for the field', () => {
    expect(OPERATORS.type).toEqual(['is'])
    expect(OPERATORS.tag).toContain('isNot')
  })
})
