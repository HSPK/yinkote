import { describe, expect, it } from 'vitest'
import { renderToStaticMarkup } from 'react-dom/server'

import { blocks, Markdown } from './markdown'

const html = (source: string) => renderToStaticMarkup(<Markdown source={source} />)

describe('markdown in an answer', () => {
  it('renders what models actually produce', () => {
    // They answer in markdown whether or not anybody asked, so showing
    // `**bold**` verbatim looks like the program is broken.
    expect(html('**bold** and *italic* and `code`')).toContain('<strong>bold</strong>')
    expect(html('**bold** and *italic* and `code`')).toContain('<em>italic</em>')
    expect(html('**bold** and *italic* and `code`')).toContain('<code>code</code>')
  })

  it('does not mistake a double marker for two single ones', () => {
    expect(html('**not italic**')).not.toContain('<em>')
  })

  it('renders both kinds of list as lists', () => {
    expect(html('- one\n- two')).toContain('<li>one</li>')
    expect(html('1. first\n2. second')).toContain('<ol')
  })

  it('keeps consecutive items in one list', () => {
    // One list per item is technically markup and visually nonsense.
    expect(html('- one\n- two\n- three').match(/<ul/g)).toHaveLength(1)
  })

  it('renders a fenced block as code', () => {
    expect(html('```rust\nlet x = 1;\n```')).toContain('let x = 1;')
    expect(html('```\nplain\n```')).toContain('<pre')
  })

  it('renders an unclosed fence, because an answer still arriving has one', () => {
    // Refusing to render until the closing fence arrives makes streaming look
    // frozen for as long as the code block takes.
    expect(html('```\nhalf a function')).toContain('half a function')
  })

  it('links to addresses a browser should follow', () => {
    expect(html('[a paper](https://example.org/p)')).toContain('href="https://example.org/p"')
    expect(html('[a paper](https://example.org/p)')).toContain('rel="noopener noreferrer"')
  })

  it('refuses a link that is not an address', () => {
    // The one thing a text renderer can still get wrong.
    const out = html('[click](javascript:alert(1))')
    expect(out).not.toContain('href')
    expect(out).toContain('click')
  })

  it('escapes rather than injects, because there is no HTML to inject into', () => {
    // Built as React nodes on purpose: an answer is text from a model, which
    // may be repeating text from a page, which may be anything.
    const out = html('<img src=x onerror=alert(1)>')
    expect(out).not.toContain('<img')
    expect(out).toContain('&lt;img')
  })

  it('treats a blank line as a paragraph break', () => {
    expect(html('one\n\ntwo').match(/<p/g)).toHaveLength(2)
  })

  it('keeps plain prose plain', () => {
    expect(html('Just a sentence.')).toBe('<p class="md-p">Just a sentence.</p>')
  })

  it('reads a heading without swallowing the hash of a tag', () => {
    expect(blocks('# Title').map((b) => b.kind)).toEqual(['h'])
    // `#tag` has no space and is not a heading.
    expect(blocks('#tag is not a heading').map((b) => b.kind)).toEqual(['p'])
  })
})
