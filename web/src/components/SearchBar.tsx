import { useEffect, useMemo, useRef, useState } from 'react'

import { useT } from '../i18n'
import { rankMatches } from '../lib/fuzzy'
import {
  addToken,
  completeTag,
  modeReason,
  parseQuery,
  pendingTag,
  removeToken,
} from '../lib/query'
import { useStore } from '../state/store'
import { Icon } from '../ui'

/** How many tag suggestions to offer; more than this is a list, not a hint. */
const SUGGESTIONS = 8

/**
 * The search box.
 *
 * Understood operators become chips as they are typed, so the query says what
 * it will do without a syntax reference beside the screen. The retrieval mode
 * is shown, not chosen — see `inferMode`.
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

  const tokens = useMemo(() => parseQuery(query), [query])
  const chips = tokens.filter((token) => token.field !== 'text')

  /** The partial `tag:` the caret is sitting in, if any. */
  const pending = useMemo(() => pendingTag(query), [query])

  const suggestions = useMemo(() => {
    if (pending === null) return []
    const chosen = new Set(chips.filter((c) => c.field === 'tag').map((c) => c.value))
    const pool = tags.map((tag) => tag.name).filter((name) => !chosen.has(name))
    return (pending ? rankMatches(pending, pool, (n) => n) : pool).slice(0, SUGGESTIONS)
  }, [pending, tags, chips])

  useEffect(() => setHighlight(0), [pending])

  const complete = (tag: string) => {
    setQuery(`${completeTag(query, tag)} `)
    inputRef.current?.focus()
  }

  const showSuggestions = focused && suggestions.length > 0

  return (
    <div className="search" data-open={showSuggestions || undefined}>
      <Icon.Search size={12} className="search-icon" />

      {chips.map((token) => {
        const index = tokens.indexOf(token)
        return (
          <button
            key={`${token.source}-${index}`}
            className="chip-token"
            data-negated={token.negated || undefined}
            title={t('search.removeToken')}
            onClick={() => setQuery(removeToken(query, index))}
          >
            <span className="chip-field">{t(`search.field.${token.field}`)}</span>
            {token.value}
          </button>
        )
      })}

      <input
        id="search-input"
        ref={inputRef}
        value={query}
        spellCheck={false}
        autoComplete="off"
        placeholder={t('search.placeholder')}
        onFocus={() => setFocused(true)}
        // A click on a suggestion must land before the list disappears.
        onBlur={() => window.setTimeout(() => setFocused(false), 120)}
        onChange={(e) => setQuery(e.target.value)}
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
          // Backspace at the start of an empty box removes the last chip,
          // which is how every tag input has worked for a decade.
          const last = chips[chips.length - 1]
          if (e.key === 'Backspace' && last && !e.currentTarget.value) {
            setQuery(removeToken(query, tokens.indexOf(last)))
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
