/**
 *  What went wrong, in the two parts a reader needs.
 *
 *  The server has always answered failures with a `code` naming the *class* of
 *  mistake and a `title` written in English. Six call sites kept only the
 *  title, so a Chinese reader was shown an English sentence -- the one thing
 *  this program says it never does.
 *
 *  A code alone is not enough either: "not found" does not say *what* was not
 *  found, and only the server's sentence does. So both are kept -- the class
 *  is translated and shown, the sentence stays available underneath.
 */
import type { MessageKey, Translate } from '../i18n'

export interface Failure {
  /** A catalogue key when the class is one we know, otherwise empty. */
  code: string
  /** The server's own words, or the browser's for a failure to connect. */
  detail: string
}

/** Codes the catalogue can name. Anything else falls back to the detail. */
export const KNOWN_CODES = new Set([
  'not_found',
  'invalid_input',
  'conflict',
  'version_conflict',
  'unauthorized',
  'forbidden',
  'storage_error',
  'search_error',
  'plugin_error',
  'unavailable',
  'internal_error',
  'method_not_allowed',
  'unsupported_media_type',
  'too_large',
  'internal',
])

export function failureOf(e: unknown): Failure {
  const detail = e instanceof Error ? e.message : String(e)
  // Read as a property rather than by `instanceof ApiError`, so this file does
  // not depend on the API client. That direction is backwards for a `lib/`
  // module, and it broke every test that mocks the client: the mock has no
  // real class to compare against, and merely touching the name throws.
  const raw = (e as { code?: unknown }).code
  const code = typeof raw === 'string' && KNOWN_CODES.has(raw) ? raw : ''
  return { code, detail }
}

/**
 *  The line to show. An unrecognised failure keeps the server's sentence,
 *  because a sentence in the wrong language still beats saying nothing.
 */
export function failureText(t: Translate, failure: Failure | null): string {
  if (!failure) return ''
  return failure.code ? t(`failure.${failure.code}` as MessageKey) : failure.detail
}
