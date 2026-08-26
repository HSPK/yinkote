import { defineConfig } from 'vitest/config'

export default defineConfig({
  // React only enables its `act` support when this flag is set, and without it
  // every render logs a warning that means nothing here.
  define: { 'globalThis.IS_REACT_ACT_ENVIRONMENT': 'true' },
  test: {
    // Most suites are pure logic and run fastest in Node; only the few that
    // touch the document opt into a DOM.
    environment: 'node',
    environmentMatchGlobs: [
      ['**/theme.test.ts', 'jsdom'],
      ['**/*.render.test.tsx', 'jsdom'],
    ],
  },
})
