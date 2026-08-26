import { describe, expect, it } from 'vitest'

import { Icon } from '../ui'
import { COLLECTION_COLOURS, COLLECTION_ICONS, collectionColour, collectionIcon } from './collections'

describe('collection appearance', () => {
  it('offers exactly five colours', () => {
    expect(COLLECTION_COLOURS).toHaveLength(5)
    expect(new Set(COLLECTION_COLOURS).size).toBe(5)
  })

  it('resolves every offered icon to a drawing that exists', () => {
    for (const name of COLLECTION_ICONS) {
      expect(collectionIcon(name), name).toBeTypeOf('function')
    }
  })

  it('falls back rather than throwing on an icon this build never heard of', () => {
    // A library edited by a newer version must still open here.
    expect(collectionIcon('hologram')).toBe(Icon.Folder)
    expect(collectionIcon(undefined)).toBe(Icon.Folder)
    expect(collectionIcon(undefined, 'Smart')).toBe(Icon.Smart)
  })

  it('accepts a known colour and rejects anything else', () => {
    expect(collectionColour('amber')).toBe('amber')
    expect(collectionColour('#ff0000')).toBeUndefined()
    expect(collectionColour(undefined)).toBeUndefined()
  })
})
