import { describe, expect, it } from 'vitest'

import { shownValue, worthSaving, type Edit } from './DetailPanel'

/**
 * An edit belongs to the paper it was made on.
 *
 * Clicking into the publication field and then selecting another paper wrote
 * the first paper's publication onto the second. One editor instance serves
 * every selection: on switching it re-rendered holding the old text and the
 * new key, and the blur that followed committed that pair. Silently, with no
 * undo, and to a field nobody was looking at — two papers in this library were
 * found carrying their neighbours' journal names.
 *
 * Tested as a rule rather than through the component, because reproducing it
 * needs a re-render to have happened and its effect not to have run yet, and
 * `act` flushes both together. A test that cannot fail is worse than no test
 * (§3.253), so the invariant is stated where it can be checked directly.
 */
describe('an edit in the detail panel', () => {
  const edit: Edit = { key: 'AAAAAAAA', value: 'Environment International' }

  it('is shown only on the paper it was made on', () => {
    expect(shownValue(edit, 'AAAAAAAA', 'Environment International')).toBe(
      'Environment International',
    )
    // Another paper is selected: the box must show that paper's own value,
    // not the text left behind by the last one.
    expect(shownValue(edit, 'BBBBBBBB', 'JAMA Pediatrics')).toBe('JAMA Pediatrics')
  })

  it('is never saved to a different paper', () => {
    expect(worthSaving(edit, 'BBBBBBBB', 'JAMA Pediatrics')).toBe(false)
  })

  it('is saved to its own paper when it actually changed something', () => {
    expect(worthSaving(edit, 'AAAAAAAA', 'Environmental Research')).toBe(true)
  })

  it('is not saved when nothing was typed', () => {
    // Clicking into a field and out of it again is not an edit, and must not
    // bump the version or the modified date.
    expect(worthSaving(edit, 'AAAAAAAA', 'Environment International')).toBe(false)
  })
})
