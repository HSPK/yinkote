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

describe('locale hygiene', () => {
  const CJK = /[\u3400-\u4dbf\u4e00-\u9fff\u3000-\u303f\uff00-\uffef]/

  it('keeps the English catalogue free of Chinese', () => {
    const leaked = Object.entries(enUS).filter(([, value]) => CJK.test(value))
    expect(leaked, `untranslated: ${leaked.map(([k]) => k).join(', ')}`).toEqual([])
  })

  it('names authors, not creators', () => {
    // Zotero's term of art is "creator"; users say "author". The label follows
    // the user, so a reintroduced "creator" is a regression, not a synonym.
    const creators = Object.entries(enUS).filter(([, v]) => /creator/i.test(v))
    expect(creators).toEqual([])
  })

  it('has no user-visible string hardcoded outside the catalogues', async () => {
    const { readdirSync, readFileSync } = await import('node:fs')
    const { join } = await import('node:path')

    const walk = (dir: string): string[] =>
      readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
        const path = join(dir, e.name)
        if (e.isDirectory()) return walk(path)
        return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : []
      })

    const offenders = walk('src')
      .filter((path) => !path.includes(join('src', 'i18n')))
      .flatMap((path) =>
        readFileSync(path, 'utf8')
          .split('\n')
          .map((line, i) => ({ where: `${path}:${i + 1}`, line }))
          .filter(({ line }) => CJK.test(line)),
      )

    expect(
      offenders,
      `hardcoded text: ${offenders.map((o) => o.where).join(', ')}`,
    ).toEqual([])
  })
})
