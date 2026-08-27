/**
 * Saving an export.
 *
 * The object URL is the part worth testing: it holds the whole file in memory
 * until it is revoked, and a library export is not small.
 */
import { afterEach, describe, expect, it, vi } from 'vitest'

import { exportName, saveText } from './download'

describe('exportName', () => {
  it('uses the extension each tool looks for', () => {
    expect(exportName('bibtex', 12)).toBe('yinkote-12-items.bib')
    expect(exportName('ris', 1)).toBe('yinkote-1-items.ris')
    expect(exportName('csljson', 3)).toBe('yinkote-3-items.json')
  })
})

describe('saveText', () => {
  afterEach(() => vi.useRealTimers())

  it('offers the file and then lets go of it', () => {
    vi.useFakeTimers()
    const created: string[] = []
    const revoked: string[] = []
    URL.createObjectURL = vi.fn(() => {
      created.push('blob:x')
      return 'blob:x'
    })
    URL.revokeObjectURL = vi.fn((url: string) => void revoked.push(url))

    saveText('out.bib', '@article{a,}')

    expect(created).toHaveLength(1)
    // Not yet: revoking in the same tick cancels the download in some browsers.
    expect(revoked).toHaveLength(0)
    vi.runAllTimers()
    expect(revoked).toEqual(['blob:x'])
  })

  it('does not leave its link in the document', () => {
    URL.createObjectURL = vi.fn(() => 'blob:x')
    URL.revokeObjectURL = vi.fn()
    saveText('out.ris', 'TY  - JOUR')
    expect(document.querySelectorAll('a[download]')).toHaveLength(0)
  })
})
