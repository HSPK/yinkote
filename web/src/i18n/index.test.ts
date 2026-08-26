import { describe, expect, it } from 'vitest'

import { detectLocale, format, LOCALES, t, useI18n } from './index'
import { enUS } from './en-US'
import { zhCN } from './zh-CN'

describe('format', () => {
  it('returns the template untouched without vars', () => {
    expect(format('plain')).toBe('plain')
  })

  it('substitutes named placeholders', () => {
    expect(format('{a} and {b}', { a: 1, b: 'two' })).toBe('1 and two')
  })

  it('repeats a placeholder used twice', () => {
    expect(format('{x}-{x}', { x: 'a' })).toBe('a-a')
  })

  it('leaves unknown placeholders alone rather than printing undefined', () => {
    expect(format('{known} {unknown}', { known: 'ok' })).toBe('ok {unknown}')
  })
})

describe('locale catalogues', () => {
  it('cover exactly the same keys', () => {
    expect(Object.keys(enUS).sort()).toEqual(Object.keys(zhCN).sort())
  })

  it('have no empty strings', () => {
    for (const [locale, dict] of [
      ['zh-CN', zhCN],
      ['en-US', enUS],
    ] as const) {
      for (const [key, value] of Object.entries(dict)) {
        expect(value.trim(), `${locale}:${key}`).not.toBe('')
      }
    }
  })

  it('use the same placeholders in every translation', () => {
    const names = (s: string) => (s.match(/\{(\w+)\}/g) ?? []).sort()
    for (const key of Object.keys(zhCN) as (keyof typeof zhCN)[]) {
      expect(names(enUS[key]), `placeholders differ for ${key}`).toEqual(names(zhCN[key]))
    }
  })

  it('is offered for every advertised locale', () => {
    expect(LOCALES.map((l) => l.value).sort()).toEqual(['en-US', 'zh-CN'])
  })
})

describe('detectLocale', () => {
  it.each([
    [['zh-CN', 'en'], 'zh-CN'],
    [['zh-Hant-TW'], 'zh-CN'],
    [['en-GB'], 'en-US'],
    [['fr-FR', 'en-US'], 'en-US'],
  ])('maps %s to %s', (languages, expected) => {
    expect(detectLocale(languages)).toBe(expected)
  })

  it('falls back when nothing matches', () => {
    expect(detectLocale(['fr', 'de'])).toBe('zh-CN')
    expect(detectLocale([])).toBe('zh-CN')
  })
})

describe('t', () => {
  it('follows the active locale', () => {
    useI18n.setState({ locale: 'zh-CN' })
    expect(t('nav.library')).toBe('文库')
    useI18n.setState({ locale: 'en-US' })
    expect(t('nav.library')).toBe('Library')
  })

  it('interpolates', () => {
    useI18n.setState({ locale: 'en-US' })
    expect(t('status.selected', { count: 3 })).toBe('3 selected')
  })
})
