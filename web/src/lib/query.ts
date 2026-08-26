/** The search query language, client side.
 *
 *  The server owns the semantics; this module owns the *shape*, so the search
 *  box can show what it understood and the smart-collection editor can build a
 *  query without either inventing a second dialect. Both compile down to the
 *  same string the server already parses, which is the only reason search and
 *  smart collections cannot drift apart.
 *
 *  Mirrors `crates/yk-search/src/parse.rs`.
 */
import type { SearchMode } from '../api/types'

export type Field = 'tag' | 'type' | 'author' | 'year' | 'text'

/** A single understood piece of the query. */
export interface Token {
  field: Field
  value: string
  /** Only meaningful for `tag`: `-tag:x` excludes. */
  negated?: boolean
  /** Only meaningful for `text`: the value must appear verbatim. */
  phrase?: boolean
  /** The exact source text, so a chip can be removed without re-serialising. */
  source: string
}

/** Operator aliases the server accepts, first entry being the canonical form.
 *
 *  The Chinese aliases are query *syntax*, accepted regardless of interface
 *  language, and must match `crates/yk-search/src/parse.rs` exactly. */
const FIELDS: Record<Field, string[]> = {
  tag: ['tag', '标签'], // i18n-exempt: query syntax, not UI text
  type: ['type', '类型'], // i18n-exempt: query syntax, not UI text
  author: ['author', 'creator', '作者'], // i18n-exempt: query syntax, not UI text
  year: ['year', '年'], // i18n-exempt: query syntax, not UI text
  text: [],
}

const ALIAS_TO_FIELD = new Map<string, Field>(
  Object.entries(FIELDS).flatMap(([field, names]) =>
    names.map((n) => [n, field as Field] as [string, Field]),
  ),
)

/** Splits on whitespace but keeps quoted runs together. */
function lex(input: string): string[] {
  const out: string[] = []
  let current = ''
  let quoted = false

  for (const ch of input) {
    if (ch === '"') {
      quoted = !quoted
      current += ch
    } else if (/\s/.test(ch) && !quoted) {
      if (current) out.push(current)
      current = ''
    } else {
      current += ch
    }
  }
  if (current) out.push(current)
  return out
}

export function parseQuery(input: string): Token[] {
  return lex(input).map((raw) => {
    const negated = raw.startsWith('-')
    const body = negated ? raw.slice(1) : raw

    if (body.startsWith('"')) {
      return { field: 'text', value: body.replace(/"/g, ''), phrase: true, source: raw }
    }

    const colon = body.indexOf(':')
    if (colon > 0) {
      const field = ALIAS_TO_FIELD.get(body.slice(0, colon).toLowerCase())
      // The server trims the quotes that let a value contain spaces.
      const value = body.slice(colon + 1).replace(/"/g, '')
      // An unknown operator is the user's text, not a mistake to swallow.
      if (field && field !== 'text' && value) return { field, value, negated, source: raw }
    }

    return { field: 'text', value: raw, source: raw }
  })
}

/** Turn tokens back into a query string. */
export function serialiseQuery(tokens: Token[]): string {
  return tokens.map(tokenSource).join(' ').trim()
}

/** The canonical source text for a token, used when building one from scratch. */
export function tokenSource(token: Token): string {
  if (token.field === 'text') {
    return token.phrase ? `"${token.value}"` : token.value
  }
  const prefix = token.negated ? '-' : ''
  const value = /\s/.test(token.value) ? `"${token.value}"` : token.value
  return `${prefix}${FIELDS[token.field][0]}:${value}`
}

/** Remove one token by position, preserving everything else verbatim. */
export function removeToken(input: string, index: number): string {
  const tokens = parseQuery(input)
  return tokens
    .filter((_, i) => i !== index)
    .map((t) => t.source)
    .join(' ')
}

/** Append a token if the query does not already constrain the same way. */
export function addToken(input: string, token: Token): string {
  const source = tokenSource(token)
  const existing = parseQuery(input)
  if (existing.some((t) => t.source === source)) return input
  return input ? `${input} ${source}` : source
}

/** Matches a half-typed tag operator at the end of the input.
 *
 *  Built from the alias table rather than written out again, so adding an
 *  alias cannot leave completion behind. */
const PENDING_TAG = new RegExp(`(^|\\s)(-?)(${FIELDS.tag.join('|')}):([^\\s"]*)$`)

/** The partially-typed tag the caret is sitting in, or `null`. */
export function pendingTag(input: string): string | null {
  return PENDING_TAG.exec(input)?.[4] ?? null
}

/** Replace the half-typed tag operator with a complete one. */
export function completeTag(input: string, tag: string): string {
  return input.replace(PENDING_TAG, (_whole, lead: string, negation: string) => {
    const token = tokenSource({ field: 'tag', value: tag, negated: negation === '-', source: '' })
    return `${lead}${token}`
  })
}

const CJK = /[\u3400-\u4dbf\u4e00-\u9fff]/

/**
 * Choose a retrieval mode from the query itself.
 *
 * Hybrid is the honest default — it fuses keyword, fuzzy and semantic — but it
 * is also the most expensive, and there are cases where the user has already
 * told us they want something exact:
 *
 * - A quoted phrase is a statement of intent. Ranking it semantically would
 *   return things that never contain it.
 * - Filters with no free text have nothing to rank; this is a lookup.
 * - One or two characters carry no semantics worth embedding, and fuzzy
 *   matching on them matches nearly everything. CJK is the exception: two
 *   characters there are a word.
 */
export function inferMode(input: string): SearchMode {
  const tokens = parseQuery(input)
  const text = tokens.filter((t) => t.field === 'text')

  if (!text.length) return 'keyword'
  if (text.some((t) => t.phrase)) return 'keyword'

  const body = text.map((t) => t.value).join('')
  if (body.length <= 2 && !CJK.test(body)) return 'keyword'

  return 'hybrid'
}

/** A short explanation of why that mode was chosen, for the mode indicator. */
export function modeReason(input: string): 'phrase' | 'filter' | 'short' | 'text' {
  const tokens = parseQuery(input)
  const text = tokens.filter((t) => t.field === 'text')
  if (!text.length) return 'filter'
  if (text.some((t) => t.phrase)) return 'phrase'
  const body = text.map((t) => t.value).join('')
  if (body.length <= 2 && !CJK.test(body)) return 'short'
  return 'text'
}

// ─── structured rules ───────────────────────────────────────────────────────

/**
 * A rule is the same query, arranged as Field / Operator / Value.
 *
 * Smart collections are edited this way but still *stored* as a query string,
 * so the retrieval pipeline stays the single implementation. Anything the rule
 * editor cannot express survives untouched in the free-text rule rather than
 * being silently rewritten.
 */
export type Operator = 'is' | 'isNot' | 'contains' | 'phrase' | 'from' | 'to' | 'between'

export interface Rule {
  field: Field
  op: Operator
  value: string
  /** Only for `between`. */
  value2?: string
}

/** Operators offered for each field, first being the default. */
export const OPERATORS: Record<Field, Operator[]> = {
  tag: ['is', 'isNot'],
  type: ['is'],
  author: ['contains'],
  year: ['is', 'from', 'to', 'between'],
  text: ['contains', 'phrase'],
}

function yearRule(value: string): Rule {
  const between = value.split('..')
  if (between.length === 2 && between[0] && between[1]) {
    return { field: 'year', op: 'between', value: between[0], value2: between[1] }
  }
  if (value.startsWith('>=')) return { field: 'year', op: 'from', value: value.slice(2) }
  if (value.startsWith('<=')) return { field: 'year', op: 'to', value: value.slice(2) }
  if (value.startsWith('>')) return { field: 'year', op: 'from', value: value.slice(1) }
  if (value.startsWith('<')) return { field: 'year', op: 'to', value: value.slice(1) }
  return { field: 'year', op: 'is', value }
}

/** Read a stored query back into editable rules. */
export function rulesFromQuery(input: string): Rule[] {
  return parseQuery(input).map((token) => {
    switch (token.field) {
      case 'tag':
        return { field: 'tag', op: token.negated ? 'isNot' : 'is', value: token.value }
      case 'year':
        return yearRule(token.value)
      case 'text':
        return { field: 'text', op: token.phrase ? 'phrase' : 'contains', value: token.value }
      default:
        return { field: token.field, op: OPERATORS[token.field][0]!, value: token.value }
    }
  })
}

function ruleToken(rule: Rule): Token | null {
  const value = rule.value.trim()
  if (!value) return null

  switch (rule.field) {
    case 'tag':
      return { field: 'tag', value, negated: rule.op === 'isNot', source: '' }
    case 'text':
      return { field: 'text', value, phrase: rule.op === 'phrase', source: '' }
    case 'year': {
      const second = rule.value2?.trim()
      const range =
        rule.op === 'between' && second
          ? `${value}..${second}`
          : rule.op === 'from'
            ? `>=${value}`
            : rule.op === 'to'
              ? `<=${value}`
              : value
      return { field: 'year', value: range, source: '' }
    }
    default:
      return { field: rule.field, value, source: '' }
  }
}

/** Compile rules into the query string the server already understands. */
export function queryFromRules(rules: Rule[]): string {
  return rules
    .map(ruleToken)
    .filter((t): t is Token => t !== null)
    .map(tokenSource)
    .join(' ')
}

/**
 * Split a half-typed query into finished operators and what is still being typed.
 *
 * The search box shows finished operators as chips and keeps only the rest in
 * the input, so the query is never displayed twice. The final token is left
 * alone unless the user has typed a space after it: promoting `tag:sur` to a
 * chip the moment it parses would make the tag impossible to finish typing.
 */
export function splitCommitted(draft: string): { committed: Token[]; rest: string } {
  const tokens = parseQuery(draft)
  const settled = /\s$/.test(draft) ? tokens.length : tokens.length - 1

  const committed: Token[] = []
  const rest: string[] = []
  tokens.forEach((token, i) => {
    if (i < settled && token.field !== 'text') committed.push(token)
    else rest.push(token.source)
  })

  // A trailing space is meaningful to the caller: it is what let the last
  // operator settle, and dropping it would re-open the one just committed.
  const tail = /\s$/.test(draft) && rest.length ? ' ' : ''
  return { committed, rest: rest.join(' ') + tail }
}

/** Flip a tag between "must have" and "must not have". */
export function negateToken(token: Token): Token {
  return { ...token, negated: !token.negated, source: '' }
}
