import { describe, expect, it } from 'vitest'

import { ApiError } from '../api/client'
import { enUS } from '../i18n/en-US'
import { zhCN } from '../i18n/zh-CN'
import { KNOWN_CODES, failureOf, failureText } from './errors'

const en = ((key: string) => enUS[key as keyof typeof enUS] ?? key) as never
const zh = ((key: string) => zhCN[key as keyof typeof zhCN] ?? key) as never

describe('what the reader is told went wrong', () => {
  it('names the class of failure in the reader’s language', () => {
    // The envelope has carried `code` since rejections got one; the banner
    // showed `title`, which is written in English at the point it is thrown.
    const failure = failureOf(new ApiError(404, 'not_found', 'not found: no such item'))
    expect(failureText(en, failure)).toBe(enUS['failure.not_found'])
    expect(failureText(zh, failure)).toBe(zhCN['failure.not_found'])
    expect(failureText(zh, failure)).not.toMatch(/[a-z]/)
  })

  it('keeps the server’s sentence, which is the part that says which thing', () => {
    // "Not found" does not say what was not found. The class goes on screen,
    // the detail stays on the element for anyone diagnosing.
    const failure = failureOf(new ApiError(404, 'not_found', 'not found: no such item'))
    expect(failure.detail).toBe('not found: no such item')
  })

  it('falls back to whatever it was given when the class is unknown', () => {
    // A network failure is a browser string with no code at all. A sentence in
    // the wrong language still beats an empty banner.
    expect(failureText(en, failureOf(new TypeError('Failed to fetch')))).toBe('Failed to fetch')
    expect(failureText(en, failureOf('something odd'))).toBe('something odd')
    // A code the catalogue does not know must not render as its own key.
    const unknown = failureOf(new ApiError(418, 'teapot', 'I am a teapot'))
    expect(failureText(en, unknown)).toBe('I am a teapot')
  })

  it('says nothing when nothing went wrong', () => {
    expect(failureText(en, null)).toBe('')
  })

  it('claims exactly the codes the catalogues carry', () => {
    // Two lists that drift silently: a claimed code with no entry renders as
    // its own key, and an entry no code claims is paid for in both languages
    // forever. The dead-entry lint cannot see either, because the key is built
    // at runtime and only the `failure.` prefix is visible to it.
    const catalogued = Object.keys(enUS)
      .filter((key) => key.startsWith('failure.'))
      .map((key) => key.slice('failure.'.length))
    expect(catalogued.sort()).toEqual([...KNOWN_CODES].sort())
  })

  it('has both languages for every code it claims', () => {
    // Two lists that must not drift: a claimed code with no entry renders as
    // the bare key, which is worse than the English it replaced.
    const failure = (code: string) => failureOf(new ApiError(500, code, 'x'))
    for (const code of ['storage_error', 'version_conflict', 'too_large', 'unavailable']) {
      expect(failureText(en, failure(code)), code).not.toBe('x')
      expect(failureText(zh, failure(code)), code).not.toBe('x')
      expect(failureText(zh, failure(code)), code).not.toMatch(/^failure\./)
    }
  })
})
