/**
 * Naming a paper in a question.
 *
 * The parsing is the part that goes wrong quietly: an `@` in an email address
 * is not a mention, and a mention that has been typed past is not one either.
 */
import { describe, expect, it } from 'vitest'

import { mentionQuery, stripMention } from './components/MentionPicker'

describe('spotting an @ mention', () => {
  it('reads the word being typed after an @', () => {
    expect(mentionQuery('what about @atten', 17)).toBe('atten')
  })

  it('starts empty the moment the @ is typed', () => {
    // The picker opens on `@` alone, showing recent papers.
    expect(mentionQuery('tell me about @', 15)).toBe('')
  })

  it('ignores an @ inside a word, so an address is not a mention', () => {
    expect(mentionQuery('mail me at bob@example.com', 26)).toBeNull()
  })

  it('closes once the user types past it', () => {
    // A space means they moved on without picking anything.
    expect(mentionQuery('@atten is all you need', 22)).toBeNull()
  })

  it('reads the mention at the caret, not the last one in the line', () => {
    const text = '@first and @second'
    expect(mentionQuery(text, 6)).toBe('first')
  })

  it('removes only the half-typed mention when one is picked', () => {
    expect(stripMention('compare @atten with the rest', 14)).toBe('compare  with the rest')
  })

  it('leaves text alone when there is nothing to strip', () => {
    expect(stripMention('no mention here', 8)).toBe('no mention here')
  })
})
