/** Naming a paper in a question.
 *
 *  Typing `@` opens a list; picking one attaches the paper to the message. The
 *  attachment travels as an item key rather than as text, so the assistant is
 *  told which paper was meant instead of searching for a title the user may
 *  have half-typed — that search costs a step and sometimes lands on a
 *  different paper with a similar name.
 *
 *  The chips below the box are the honest part of the interface: what is
 *  attached is visible and removable before the question is sent, rather than
 *  being inferred from the prose afterwards.
 */
import { useCallback, useEffect, useRef, useState } from 'react'

import { api } from '../api/client'
import type { Item } from '../api/types'
import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Icon } from '../ui'

/** Long enough not to search on every keystroke, short enough to feel live. */
const DEBOUNCE_MS = 120
const LIMIT = 8

/** The `@word` being typed at the caret, if there is one. */
export function mentionQuery(text: string, caret: number): string | null {
  const before = text.slice(0, caret)
  const at = before.lastIndexOf('@')
  if (at < 0) return null
  // Only at a word boundary, so an email address is not a mention.
  if (at > 0 && !/\s/.test(before[at - 1] ?? '')) return null
  const word = before.slice(at + 1)
  // A space ends it: the user moved on without picking anything.
  return /\s/.test(word) ? null : word
}

/** Replace the `@word` at the caret with nothing, leaving the rest intact. */
export function stripMention(text: string, caret: number): string {
  const before = text.slice(0, caret)
  const at = before.lastIndexOf('@')
  if (at < 0) return text
  return before.slice(0, at) + text.slice(caret)
}

export function MentionPicker({
  query,
  onPick,
  onDismiss,
}: {
  query: string
  onPick: (item: Item) => void
  onDismiss: () => void
}) {
  const t = useT()
  const library = useStore((s) => s.library)
  const [items, setItems] = useState<Item[]>([])
  const [cursor, setCursor] = useState(0)
  const latest = useRef(0)

  useEffect(() => {
    const run = ++latest.current
    const timer = window.setTimeout(() => {
      void api.items
        .list(library, {
          q: query || undefined,
          mode: query ? 'hybrid' : undefined,
          limit: LIMIT,
          sort: query ? undefined : 'dateModified',
        })
        .then((page) => {
          // Out-of-order responses would otherwise show results for a query
          // the user has already typed past.
          if (latest.current === run) {
            setItems(page.items)
            setCursor(0)
          }
        })
        .catch(() => {
          if (latest.current === run) setItems([])
        })
    }, DEBOUNCE_MS)
    return () => window.clearTimeout(timer)
  }, [library, query])

  const move = useCallback(
    (delta: number) => setCursor((c) => Math.max(0, Math.min(items.length - 1, c + delta))),
    [items.length],
  )

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'ArrowDown') {
        e.preventDefault()
        move(1)
      } else if (e.key === 'ArrowUp') {
        e.preventDefault()
        move(-1)
      } else if (e.key === 'Enter' || e.key === 'Tab') {
        const chosen = items[cursor]
        if (chosen) {
          e.preventDefault()
          e.stopPropagation()
          onPick(chosen)
        }
      } else if (e.key === 'Escape') {
        e.preventDefault()
        onDismiss()
      }
    }
    // Capture, so Enter picks a paper instead of sending the message.
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [items, cursor, move, onPick, onDismiss])

  if (!items.length) {
    return (
      <div className="mention-popup">
        <div className="mention-empty dim">{t('chat.mentionNone')}</div>
      </div>
    )
  }

  return (
    <div className="mention-popup" role="listbox">
      {items.map((item, i) => (
        <button
          key={item.key}
          role="option"
          aria-selected={i === cursor}
          className="mention-row"
          data-active={i === cursor}
          onMouseEnter={() => setCursor(i)}
          onMouseDown={(e) => {
            // Before blur, or the textarea loses the caret first.
            e.preventDefault()
            onPick(item)
          }}
        >
          <Icon.Library className="glyph" size={11} />
          <span className="mention-title">{item.title || item.key}</span>
          <span className="mention-meta dim">
            {[item.creators?.[0]?.lastName, item.date?.slice(0, 4)].filter(Boolean).join(' · ')}
          </span>
        </button>
      ))}
    </div>
  )
}
