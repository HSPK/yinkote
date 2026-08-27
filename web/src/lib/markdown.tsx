/** A small markdown renderer.
 *
 *  Models answer in markdown whether or not anybody asked them to, so an
 *  assistant that shows `**bold**` and `- item` verbatim looks broken. This
 *  covers what they actually produce: headings, emphasis, code, code fences,
 *  lists, block quotes and links.
 *
 *  It builds React nodes rather than HTML. That is the whole reason it is
 *  hand-written instead of a dependency: an answer is text from a model, which
 *  may be repeating text from a web page, which may be repeating anything at
 *  all. With no `dangerouslySetInnerHTML` there is no injection to reason
 *  about, and the renderer cannot become a security question later.
 *
 *  What it deliberately does not do: tables, footnotes, nested lists, inline
 *  HTML. Each would roughly double this file, and none of them shows up in an
 *  answer about a library.
 */
import type { ReactNode } from 'react'

/** Inline spans: code, bold, italic, links. */
export function inline(text: string, keyPrefix = ''): ReactNode[] {
  const out: ReactNode[] = []
  // One pass, longest markers first, so `**` is never mistaken for two `*`.
  const pattern = /(`[^`]+`)|(\*\*[^*]+\*\*)|(__[^_]+__)|(\*[^*]+\*)|(\[([^\]]+)\]\(([^)\s]+)\))/g

  let last = 0
  let match: RegExpExecArray | null
  let n = 0

  while ((match = pattern.exec(text))) {
    if (match.index > last) out.push(text.slice(last, match.index))
    const key = `${keyPrefix}i${n++}`

    if (match[1]) out.push(<code key={key}>{match[1].slice(1, -1)}</code>)
    else if (match[2]) out.push(<strong key={key}>{match[2].slice(2, -2)}</strong>)
    else if (match[3]) out.push(<strong key={key}>{match[3].slice(2, -2)}</strong>)
    else if (match[4]) out.push(<em key={key}>{match[4].slice(1, -1)}</em>)
    else if (match[5]) {
      const href = match[7] ?? ''
      // Only addresses a browser should follow. `javascript:` in a link is the
      // one thing a text renderer can still get wrong.
      const safe = /^(https?:|mailto:|doi:)/i.test(href)
      out.push(
        safe ? (
          <a key={key} href={href} target="_blank" rel="noopener noreferrer">
            {match[6]}
          </a>
        ) : (
          <span key={key}>{match[6]}</span>
        ),
      )
    }
    last = match.index + match[0].length
  }

  if (last < text.length) out.push(text.slice(last))
  return out
}

interface Block {
  kind: 'p' | 'h' | 'ul' | 'ol' | 'code' | 'quote'
  level?: number
  lines: string[]
  language?: string
}

/** Split into blocks, which is all the structure that matters here. */
export function blocks(source: string): Block[] {
  const out: Block[] = []
  const lines = source.replace(/\r\n?/g, '\n').split('\n')
  let i = 0

  const push = (block: Block) => {
    const last = out[out.length - 1]
    // Consecutive list items are one list, not one list each.
    if (last && last.kind === block.kind && (block.kind === 'ul' || block.kind === 'ol')) {
      last.lines.push(...block.lines)
    } else {
      out.push(block)
    }
  }

  while (i < lines.length) {
    const line = lines[i]!

    // A fence, which runs until it closes *or until the text ends* — an answer
    // still arriving is nearly always mid-fence, and refusing to render it
    // until the closing ``` would make streaming look frozen.
    const fence = /^```(\w*)\s*$/.exec(line)
    if (fence) {
      const body: string[] = []
      i += 1
      while (i < lines.length && !/^```\s*$/.test(lines[i]!)) {
        body.push(lines[i]!)
        i += 1
      }
      i += 1
      out.push({ kind: 'code', lines: body, language: fence[1] || undefined })
      continue
    }

    const heading = /^(#{1,4})\s+(.*)$/.exec(line)
    if (heading) {
      out.push({ kind: 'h', level: heading[1]!.length, lines: [heading[2]!] })
      i += 1
      continue
    }

    const bullet = /^\s*[-*+]\s+(.*)$/.exec(line)
    if (bullet) {
      push({ kind: 'ul', lines: [bullet[1]!] })
      i += 1
      continue
    }

    const numbered = /^\s*\d+[.)]\s+(.*)$/.exec(line)
    if (numbered) {
      push({ kind: 'ol', lines: [numbered[1]!] })
      i += 1
      continue
    }

    const quote = /^>\s?(.*)$/.exec(line)
    if (quote) {
      push({ kind: 'quote', lines: [quote[1]!] })
      i += 1
      continue
    }

    if (!line.trim()) {
      i += 1
      continue
    }

    // A paragraph runs to the next blank line or structural line.
    const paragraph: string[] = []
    while (i < lines.length) {
      const next = lines[i]!
      if (!next.trim() || /^(#{1,4}\s|```|>\s?|\s*[-*+]\s|\s*\d+[.)]\s)/.test(next)) break
      paragraph.push(next)
      i += 1
    }
    out.push({ kind: 'p', lines: paragraph })
  }

  return out
}

/** Render markdown as React nodes. */
export function Markdown({ source }: { source: string }) {
  return (
    <>
      {blocks(source).map((block, b) => {
        const key = `b${b}`
        switch (block.kind) {
          case 'code':
            return (
              <pre key={key} className="md-code" data-language={block.language}>
                {block.lines.join('\n')}
              </pre>
            )
          case 'h': {
            const Tag = (`h${Math.min(block.level ?? 1, 4) + 2}` as 'h3')
            return (
              <Tag key={key} className="md-h">
                {inline(block.lines[0] ?? '', key)}
              </Tag>
            )
          }
          case 'ul':
          case 'ol': {
            const Tag = block.kind
            return (
              <Tag key={key} className="md-list">
                {block.lines.map((line, i) => (
                  <li key={i}>{inline(line, `${key}-${i}`)}</li>
                ))}
              </Tag>
            )
          }
          case 'quote':
            return (
              <blockquote key={key} className="md-quote">
                {inline(block.lines.join(' '), key)}
              </blockquote>
            )
          default:
            return (
              <p key={key} className="md-p">
                {inline(block.lines.join('\n'), key)}
              </p>
            )
        }
      })}
    </>
  )
}
