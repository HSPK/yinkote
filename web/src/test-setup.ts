/**
 * What every test can assume before it starts.
 *
 * The locale, and only the locale. `detectLocale` falls back to Chinese for a
 * language it does not recognise, which is right for the product and wrong to
 * inherit in a test: assertions are written against the English catalogue, and
 * which one they got depended on whether the runtime happened to provide
 * `navigator.languages`. Node 22 does, Node 20 does not, so the suite passed
 * on this machine and failed in CI, where every assertion against an English
 * message met the Chinese one instead.
 *
 * Pinned rather than guarded, so a test that is *about* another language still
 * sets one for itself and says so.
 */
import { beforeEach } from 'vitest'

import { useI18n } from './i18n'

beforeEach(() => {
  useI18n.setState({ locale: 'en-US' })
})
