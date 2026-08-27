/** A way through a long conversation.
 *
 *  A thread with three hundred messages in it cannot be navigated by
 *  scrolling: what somebody is looking for is a question they asked, and the
 *  answers between are what makes finding it hard. So the rail marks the
 *  questions — one tick each — and hovering one shows what it said.
 *
 *  It appears only when the conversation is long enough to need it. On a
 *  six-message thread it would be chrome explaining a problem nobody has.
 */
import { useState } from 'react'

import type { Message } from '../api/types'

/** Below this a conversation is short enough to scroll. */
export const RAIL_THRESHOLD = 12

/** How much of a question to show on hover. */
const PREVIEW_CHARS = 140

export interface RailMark {
  index: number
  text: string
}

/** The questions in a thread, with the position of each. */
export function railMarks(messages: Message[]): RailMark[] {
  return messages.flatMap((m, index) =>
    m.role === 'user' && m.content.trim()
      ? [{ index, text: clip(m.content.trim(), PREVIEW_CHARS) }]
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
  const [hover, setHover] = useState<RailMark | null>(null)

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
          aria-label={mark.text}
          onMouseEnter={() => setHover(mark)}
          onFocus={() => setHover(mark)}
          onClick={() => onJump(mark.index)}
        />
      ))}
      {hover && <div className="jump-preview">{hover.text}</div>}
    </div>
  )
}
