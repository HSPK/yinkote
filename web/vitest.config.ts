import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    // Most suites are pure logic and run fastest in Node; the router talks to
    // location/history, so it opts into a DOM.
    environment: 'node',
    environmentMatchGlobs: [['**/router.test.ts', 'jsdom']],
  },
})
