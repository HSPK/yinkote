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

  // One walker and one exemption rule for both checks below. An exemption may
  // sit on the offending line or the one above it, because a JSX comment does
  // not fit inside a tag.
  const sources = async () => {
    const { readdirSync, readFileSync } = await import('node:fs')
    const { join } = await import('node:path')
    const walk = (dir: string): string[] =>
      readdirSync(dir, { withFileTypes: true }).flatMap((e) => {
        const path = join(dir, e.name)
        if (e.isDirectory()) return walk(path)
        return /\.tsx?$/.test(e.name) && !/\.test\.tsx?$/.test(e.name) ? [path] : []
      })
    return walk('src')
      .filter((path) => !path.includes(join('src', 'i18n')))
      .map((path) => ({ path, lines: readFileSync(path, 'utf8').split('\n') }))
  }

  const EXEMPT = /i18n-exempt:\s*\S/
  const exempt = (lines: string[], i: number) =>
    EXEMPT.test(lines[i] ?? '') || EXEMPT.test(lines[i - 1] ?? '')

  it('has no user-visible string hardcoded outside the catalogues', async () => {
    // Some Chinese is syntax, not prose: the query language accepts `标签:` as
    // an alias for `tag:` in either interface language. Such a line must say so
    // and say why, so the exemption stays deliberate rather than habitual.
    const offenders = (await sources()).flatMap(({ path, lines }) =>
      lines
        .map((line, i) => ({ where: `${path}:${i + 1}`, line, i }))
        .filter(({ line, i }) => CJK.test(line) && !exempt(lines, i)),
    )

    expect(
      offenders,
      `hardcoded text: ${offenders.map((o) => o.where).join(', ')}`,
    ).toEqual([])
  })

  it('has no catalogue entry that nothing asks for', async () => {
    // A dead key is paid for twice, once per language, and there is no way to
    // notice one by using the app. Thirty-one had accumulated.
    //
    // The subtlety is that plenty of keys are never written out in full:
    // `t(`connector.state.${state}`)` builds the key at runtime. The literal
    // half still pins a prefix, so those prefixes are collected first and any
    // key underneath one counts as reached. Without that this check would
    // condemn eighteen whole families of perfectly live entries.
    const used = (await sources()).map(({ lines }) => lines.join('\n')).join('\n')
    const prefixes = [
      ...new Set([...used.matchAll(/[`'"]([A-Za-z][\w.]*\.)\$\{/g)].map((m) => m[1] ?? '')),
    ]

    const dead = Object.keys(enUS).filter(
      (key) => !used.includes(key) && !prefixes.some((p) => key.startsWith(p)),
    )
    expect(dead, `unused catalogue entries: ${dead.join(', ')}`).toEqual([])
  })

  it('has no hardcoded English prose in markup either', async () => {
    // The check above only ever caught Chinese, which is the *unlikely*
    // accident: JSX is written in English, so `<Button>Save</Button>` sailed
    // past every test we had. This is the same rule applied to the language
    // the code is actually written in.
    //
    // Two places prose hides: text nodes, and the handful of attributes a user
    // can actually read.
    const TYPE_NOISE = /^(void|Promise|string|number|boolean|null|undefined|unknown|never|any|React|JSX)\b/
    // The application's own name is not translated in any locale, and it
    // recurs, so it is a rule rather than a scattering of exemptions.
    const PRODUCT = /^yinkote$/i
    // Examples of machine input — a URL to paste, a colour to type — read the
    // same in every language.
    const MACHINE = /^(https?:\/\/|#[0-9a-fA-F]{3,8}$)/

    const offenders: string[] = []
    for (const { path, lines } of await sources()) {
      if (!path.endsWith('.tsx')) continue
      lines.forEach((line, i) => {
        if (exempt(lines, i)) return
        // `<code>` is syntax by construction; query examples live there.
        if (!line.includes('<code>')) {
          // Text between tags. The lookbehind keeps `=>` in a type annotation
          // from reading as the end of an element.
          for (const m of line.matchAll(/(?<![=!<>-])>([^<>{}()|]*[A-Za-z]{2,}[^<>{}()|]*)</g)) {
            const text = (m[1] ?? '').trim()
            if (!text || TYPE_NOISE.test(text) || PRODUCT.test(text)) continue
            offenders.push(`${path}:${i + 1} |${text}|`)
          }
        }
        for (const m of line.matchAll(/\b(placeholder|title|aria-label|alt)="([^"]{2,})"/g)) {
          const value = m[2] ?? ''
          if (MACHINE.test(value)) continue
          offenders.push(`${path}:${i + 1} ${m[1]}="${value}"`)
        }
      })
    }

    expect(offenders, `hardcoded English: ${offenders.join(', ')}`).toEqual([])
  })
})
