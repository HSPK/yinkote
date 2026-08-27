import { describe, expect, it } from 'vitest'

import { SIDELOAD, detectPlatform } from './WordAddin'

describe('the Word add-in card', () => {
  it('guesses the platform from the user agent', () => {
    expect(detectPlatform('Mozilla/5.0 (Windows NT 10.0; Win64; x64)')).toBe('windows')
    expect(detectPlatform('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)')).toBe('mac')
    expect(detectPlatform('Mozilla/5.0 (X11; Linux x86_64)')).toBe('other')
  })

  it('offers a path for every platform it can report', () => {
    // A platform with no path would render `undefined` into the instructions,
    // which is worse than showing the generic folder.
    for (const agent of ['Windows NT', 'Macintosh', 'Linux', 'iPhone']) {
      expect(SIDELOAD[detectPlatform(agent)].path).toBeTruthy()
    }
  })

  it('gives Windows a share path, because a plain folder is not trusted', () => {
    // Word's trusted-catalogue setting takes a network share; pointing it at a
    // local directory produces an add-in that never appears in the Ribbon.
    expect(SIDELOAD.windows.path.startsWith('\\\\')).toBe(true)
  })
})
