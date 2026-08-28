import { describe, expect, it } from 'vitest'

import { snippetParts } from '../lib/format'
import { sourceFiles } from '../test/sources'

/**
 * Untrusted text must never become markup.
 *
 * "Untrusted" is not hypothetical here. A title arrives from a scraped web
 * page, an imported `.bib`, or somebody else's Zotero library, and it is shown
 * in search results with the server's `<mark>` highlighting around it. The
 * server does not escape what it puts in a snippet — asked for `alert`, it
 * answers `<script><mark>alert</mark>(1)</script>` verbatim.
 *
 * That is safe for exactly one reason: nothing renders it as HTML. React
 * escapes every string it is handed as a child, so the whole defence is the
 * absence of one API.
 */
describe('untrusted text stays text', () => {
  const HOSTILE = '<script>alert(1)</script> & <img src=x onerror=alert(2)>'

  it('never uses dangerouslySetInnerHTML anywhere', () => {
    // The load-bearing rule, and until now only a comment. One call added in
    // any component would turn every paper title in the library into script
    // the workbench runs — with the API, the file system and the plugin host
    // all one fetch away.
    const offenders = sourceFiles()
      .flatMap(({ path, lines }) =>
        lines
          .map((line, i) => ({ where: `${path}:${i + 1}`, line }))
          // Usage, not mention: the two comments explaining why this API is
          // avoided must not be read as uses of it.
          .filter(({ line }) => /dangerouslySetInnerHTML\s*[=:]|\.innerHTML\s*=/.test(line)),
      )
      .map((o) => o.where)

    expect(offenders, `renders raw HTML: ${offenders.join(', ')}`).toEqual([])
  })

  it('hands hostile markup back as inert text, neither parsed nor stripped', () => {
    // Stripping would be the wrong fix as well as an unnecessary one: a paper
    // really can be called "A <sup>13</sup>C study", and the user should see
    // the title they have, not a laundered one.
    const parts = snippetParts(`before <mark>hit</mark> ${HOSTILE}`)
    expect(parts.filter((p) => p.mark).map((p) => p.text)).toEqual(['hit'])
    expect(parts.map((p) => p.text).join('')).toBe(`before hit ${HOSTILE}`)
  })

  it('treats a title that contains <mark> as its own text, not as highlighting', () => {
    // The one case the splitter could get wrong in the user's favour: it must
    // not matter for safety, and it does not — both halves come back as text.
    const parts = snippetParts('<mark>real</mark> and <mark>fake</mark>')
    expect(parts.filter((p) => p.mark)).toHaveLength(2)
    expect(parts.every((p) => typeof p.text === 'string')).toBe(true)
  })
})
