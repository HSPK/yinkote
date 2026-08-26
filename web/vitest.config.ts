import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    // Most suites are pure logic and run fastest in Node; only the few that
    // touch the document opt into a DOM.
    environment: 'node',
    environmentMatchGlobs: [
      ['**/theme.test.ts', 'jsdom'],
    ],
  },
})
