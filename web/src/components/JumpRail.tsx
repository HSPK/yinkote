/** A way through a long conversation.
 *
 *  A thread with three hundred messages in it cannot be navigated by
 *  scrolling: what somebody is looking for is a question they asked, and the
 *  answers between are what makes finding it hard. So the rail marks the
 *  questions — one tick each — and hovering one shows what it said.
 *
 *  Always present, once there is anything to point at. It was hidden below a
 *  dozen messages, which meant it appeared without warning mid-conversation —
 *  a control that comes and goes is harder to rely on than one that is always
 *  in the same place.
 */
import { useState } from 'react'

import type { Message } from '../api/types'

/** How much of a question to show on hover. */
const PREVIEW_CHARS = 140

export interface RailMark {
  index: number
  text: string
  /** When it was asked. Two similar questions are told apart by when. */
  at: number
}

/** The questions in a thread, with the position of each. */
export function railMarks(messages: Message[]): RailMark[] {
  return messages.flatMap((m, index) =>
    m.role === 'user' && m.content.trim()
      ? [{ index, text: clip(m.content.trim(), PREVIEW_CHARS), at: m.createdAt }]
      : [],
  )
}

function clip(text: string, limit: number): string {
  const flat = text.replace(/\s+/g, ' ')
  return [...flat].length > limit ? `${[...flat].slice(0, limit).join('')}…` : flat
}

export function JumpRail({
  marks,
  active,
  onJump,
}: {
  marks: RailMark[]
  /** The message currently at the top of the view, so the rail says where you are. */
  active: number
  onJump: (index: number) => void
}) {
  const [hover, setHover] = useState<{ mark: RailMark; top: number } | null>(null)

  if (!marks.length) return null

  // The nearest question at or above the viewport: the one being read.
  const current = marks.reduce(
    (best, m) => (m.index <= active && m.index >= best ? m.index : best),
    marks[0]!.index,
  )

  return (
    <div className="jump-rail" onMouseLeave={() => setHover(null)}>
      {marks.map((mark) => (
        <button
          key={mark.index}
          className="jump-tick"
          data-current={mark.index === current || undefined}
          data-hovered={hover?.mark.index === mark.index || undefined}
          aria-label={mark.text}
          onMouseEnter={(e) => setHover({ mark, top: e.currentTarget.offsetTop })}
          onFocus={(e) => setHover({ mark, top: e.currentTarget.offsetTop })}
          onClick={() => onJump(mark.index)}
        />
      ))}
      {/* To the left of the rail, because the rail is against the right edge
          and a preview that opened rightwards would be off screen. */}
      {hover && (
        <div className="jump-preview" style={{ top: hover.top }}>
          <span className="jump-preview-time">{shortTime(hover.mark.at)}</span>
          <span className="jump-preview-text">{hover.mark.text}</span>
        </div>
      )}
    </div>
  )
}

/** The time a question was asked, at the resolution that tells two apart. */
function shortTime(at: number): string {
  if (!at) return ''
  const when = new Date(at)
  const today = new Date()
  const sameDay =
    when.getFullYear() === today.getFullYear() &&
    when.getMonth() === today.getMonth() &&
    when.getDate() === today.getDate()

  const time = `${String(when.getHours()).padStart(2, '0')}:${String(when.getMinutes()).padStart(2, '0')}`
  // A date on everything is noise in a thread from this morning; a time alone
  // is a lie in one that has been going for a week.
  return sameDay ? time : `${when.getMonth() + 1}/${when.getDate()} ${time}`
}
