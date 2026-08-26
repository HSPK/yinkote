import { beforeEach, describe, expect, it } from 'vitest'

import { applyTheme, DEFAULT_THEME, findTheme, isHexColour, THEMES } from './theme'

describe('theme catalogue', () => {
  it('has a usable default', () => {
    expect(findTheme(DEFAULT_THEME).id).toBe(DEFAULT_THEME)
  })

  it('falls back for an unknown id rather than blanking the UI', () => {
    expect(findTheme('does-not-exist')).toBe(THEMES[0])
  })

  it('defines the same variables in every theme', () => {
    const reference = Object.keys(THEMES[0]!.vars).sort()
    for (const theme of THEMES) {
      expect(Object.keys(theme.vars).sort(), theme.id).toEqual(reference)
    }
  })

  it('uses six-digit hex everywhere, so shading can parse it', () => {
    for (const theme of THEMES) {
      for (const [key, value] of Object.entries(theme.vars)) {
        expect(isHexColour(value), `${theme.id} ${key} = ${value}`).toBe(true)
      }
    }
  })

  it('offers at least one light theme', () => {
    expect(THEMES.some((t) => !t.dark)).toBe(true)
  })
})

describe('isHexColour', () => {
  it.each([
    ['#4da3ff', true],
    ['#FFFFFF', true],
    ['#fff', false],
    ['4da3ff', false],
    ['', false],
    ['rgb(1,2,3)', false],
  ])('%s -> %s', (value, expected) => {
    expect(isHexColour(value)).toBe(expected)
  })
})

describe('applyTheme', () => {
  const read = (name: string) => document.documentElement.style.getPropertyValue(name)

  beforeEach(() => {
    document.documentElement.removeAttribute('style')
    delete document.documentElement.dataset.theme
  })

  it('writes the theme variables onto the root', () => {
    applyTheme('industrial')
    expect(read('--bg')).toBe(findTheme('industrial').vars['--bg'])
    expect(document.documentElement.dataset.theme).toBe('industrial')
  })

  it('switches cleanly between themes', () => {
    applyTheme('industrial')
    applyTheme('paper')
    expect(read('--bg')).toBe(findTheme('paper').vars['--bg'])
    expect(read('color-scheme')).toBe('light')
  })

  it('overrides the accent and derives its companion shade', () => {
    applyTheme('industrial', '#ff8800')
    expect(read('--accent')).toBe('#ff8800')
    const dim = read('--accent-dim')
    expect(dim).not.toBe('#ff8800')
    expect(isHexColour(dim)).toBe(true)
  })

  it('ignores a malformed accent rather than breaking the palette', () => {
    applyTheme('industrial', 'not-a-colour')
    expect(read('--accent')).toBe(findTheme('industrial').vars['--accent'])
  })
})
