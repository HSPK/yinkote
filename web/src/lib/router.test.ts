import { afterEach, describe, expect, it, vi } from 'vitest'

import { isPage, navigate, onNavigate, pageFromHash } from './router'

afterEach(() => {
  location.hash = ''
  vi.restoreAllMocks()
})

describe('pageFromHash', () => {
  it.each([
    ['#/library', 'library'],
    ['#/settings', 'settings'],
    ['#plugins', 'plugins'],
    ['#/status?x=1', 'status'],
  ])('parses %s', (hash, expected) => {
    expect(pageFromHash(hash)).toBe(expected)
  })

  it('falls back to the library for anything unrecognised', () => {
    expect(pageFromHash('')).toBe('library')
    expect(pageFromHash('#/nope')).toBe('library')
    expect(pageFromHash('#/../etc/passwd')).toBe('library')
  })
})

describe('isPage', () => {
  it('accepts known pages only', () => {
    expect(isPage('chat')).toBe(true)
    expect(isPage('nonsense')).toBe(false)
  })
})

describe('navigate', () => {
  it('writes the hash', () => {
    navigate('settings')
    expect(location.hash).toBe('#/settings')
  })

  it('does not rewrite the hash when already there, so history stays clean', () => {
    location.hash = '#/status'
    const before = location.hash
    navigate('status')
    expect(location.hash).toBe(before)
  })
})

describe('onNavigate', () => {
  it('reports the new page and unsubscribes cleanly', () => {
    const seen: string[] = []
    const off = onNavigate((p) => seen.push(p))

    location.hash = '#/plugins'
    window.dispatchEvent(new Event('hashchange'))
    expect(seen).toEqual(['plugins'])

    off()
    location.hash = '#/settings'
    window.dispatchEvent(new Event('hashchange'))
    expect(seen).toEqual(['plugins'])
  })
})
