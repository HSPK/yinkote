import { defineConfig } from 'vitest/config'

export default defineConfig({
  // React only enables its `act` support when this flag is set, and without it
  // every render logs a warning that means nothing here.
  define: { 'globalThis.IS_REACT_ACT_ENVIRONMENT': 'true' },
  test: {
    // Most suites are pure logic and run fastest in Node; only the few that
    // touch the document opt into a DOM.
    environment: 'node',
    // One place decides the locale, so no test depends on whether the runtime
    // it happens to be on provides `navigator`.
    setupFiles: ['src/test-setup.ts'],
    // A suffix rather than a list of filenames: `theme.test.ts` had to be
    // named here individually, and the next test that touches `document`
    // would have had to be too — where the symptom is `document is not
    // defined` and the cause is a config file nobody thought to look at.
    environmentMatchGlobs: [
      ['**/theme.test.ts', 'jsdom'],
      ['**/*.dom.test.ts', 'jsdom'],
      ['**/*.render.test.tsx', 'jsdom'],
    ],
  },
})
