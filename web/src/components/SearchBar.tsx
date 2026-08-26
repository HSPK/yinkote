import { useEffect, useMemo, useRef, useState } from 'react'

import { useT } from '../i18n'
import { rankMatches } from '../lib/fuzzy'
import {
  addToken,
  completeTag,
  modeReason,
  negateToken,
  parseQuery,
  pendingTag,
  splitCommitted,
  tokenSource,
  type Token,
} from '../lib/query'
import { useStore } from '../state/store'
import { Icon } from '../ui'

/** How many tag suggestions to offer; more than this is a list, not a hint. */
const SUGGESTIONS = 8

/**
 * The search box.
 *
 * Finished operators become chips and leave the input, so the query is shown
 * once rather than twice. Clicking a chip flips it between required and
 * excluded — a useful thing to want and a safe thing to hit by accident;
 * removal is the explicit × beside it. The retrieval mode is shown, not chosen.
 */
export function SearchBar() {
  const t = useT()
  const query = useStore((s) => s.query)
  const mode = useStore((s) => s.mode)
  const tags = useStore((s) => s.tags)
  const setQuery = useStore((s) => s.setQuery)

  const inputRef = useRef<HTMLInputElement>(null)
  const [focused, setFocused] = useState(false)
  const [highlight, setHighlight] = useState(0)

  /** Operator chips, and the free text still in the input, split from the store. */
  const { chips, draft } = useMemo(() => {
    const tokens = parseQuery(query)
    return {
      chips: tokens.filter((token) => token.field !== 'text'),
      draft: tokens
        .filter((token) => token.field === 'text')
        .map((token) => token.source)
        .join(' '),
    }
  }, [query])

  /** Rebuild the whole query from chips plus whatever is in the input. */
  const compose = (nextChips: Token[], text: string) =>
    [...nextChips.map(tokenSource), text].filter(Boolean).join(' ')

  const onType = (value: string) => {
    // Anything the user has finished typing moves out of the input and becomes
    // a chip, so the two never show the same thing.
    const { committed, rest } = splitCommitted(value)
    setQuery(compose([...chips, ...committed], rest))
  }

  const pending = useMemo(() => pendingTag(draft), [draft])

  const suggestions = useMemo(() => {
    if (pending === null) return []
    const chosen = new Set(chips.filter((c) => c.field === 'tag').map((c) => c.value))
    const pool = tags.map((tag) => tag.name).filter((name) => !chosen.has(name))
    return (pending ? rankMatches(pending, pool, (n) => n) : pool).slice(0, SUGGESTIONS)
  }, [pending, tags, chips])

  useEffect(() => setHighlight(0), [pending])

  const complete = (tag: string) => {
    onType(`${completeTag(draft, tag)} `)
    inputRef.current?.focus()
  }

  const replaceChip = (index: number, next: Token | null) =>
    setQuery(
      compose(
        next ? chips.map((c, i) => (i === index ? next : c)) : chips.filter((_, i) => i !== index),
        draft,
      ),
    )

  const showSuggestions = focused && suggestions.length > 0

  return (
    <div className="search" data-open={showSuggestions || undefined}>
      <Icon.Search size={12} className="search-icon" />

      {chips.map((token, i) => (
        <span
          key={`${token.source}-${i}`}
          className="chip-token"
          data-negated={token.negated || undefined}
        >
          <button
            className="chip-body"
            title={token.field === 'tag' ? t('search.toggleToken') : token.value}
            onClick={() => token.field === 'tag' && replaceChip(i, negateToken(token))}
          >
            <span className="chip-field">{t(`search.field.${token.field}`)}</span>
            {token.value}
          </button>
          <button
            className="chip-remove"
            title={t('search.removeToken')}
            onClick={() => replaceChip(i, null)}
          >
            <Icon.Close size={9} />
          </button>
        </span>
      ))}

      <input
        id="search-input"
        ref={inputRef}
        value={draft}
        spellCheck={false}
        autoComplete="off"
        placeholder={chips.length ? '' : t('search.placeholder')}
        onFocus={() => setFocused(true)}
        // A click on a suggestion must land before the list disappears.
        onBlur={() => window.setTimeout(() => setFocused(false), 120)}
        onChange={(e) => onType(e.target.value)}
        onKeyDown={(e) => {
          if (showSuggestions) {
            if (e.key === 'ArrowDown') {
              e.preventDefault()
              return setHighlight((h) => (h + 1) % suggestions.length)
            }
            if (e.key === 'ArrowUp') {
              e.preventDefault()
              return setHighlight((h) => (h - 1 + suggestions.length) % suggestions.length)
            }
            if (e.key === 'Enter' || e.key === 'Tab') {
              e.preventDefault()
              return complete(suggestions[highlight] ?? suggestions[0]!)
            }
          }
          if (e.key === 'Escape') {
            setQuery('')
            e.currentTarget.blur()
          }
          // Backspace in an empty box removes the last chip, which is how every
          // tag input has worked for a decade.
          if (e.key === 'Backspace' && !draft && chips.length) {
            replaceChip(chips.length - 1, null)
          }
        }}
      />

      {query && (
        <span className="search-mode" title={t(`search.why.${modeReason(query)}`)}>
          {t(`search.mode.${mode}`)}
        </span>
      )}

      {showSuggestions && (
        <div className="search-suggest">
          {suggestions.map((tag, i) => (
            <button
              key={tag}
              data-active={i === highlight}
              onMouseEnter={() => setHighlight(i)}
              onMouseDown={(e) => e.preventDefault()}
              onClick={() => complete(tag)}
            >
              {tag}
            </button>
          ))}
        </div>
      )}
    </div>
  )
}

/** Add a tag filter to the current query, used by the sidebar and menus. */
export function filterByTag(tag: string): void {
  const { query, setQuery } = useStore.getState()
  setQuery(addToken(query, { field: 'tag', value: tag, source: '' }))
}
