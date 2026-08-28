/**
 * An API client that never answers.
 *
 * Page tests mount a component and assert on what it draws before any request
 * comes back, so what they need is a client whose every call is a promise that
 * stays pending. Every one of them was writing its own, and one wrote it a
 * level too shallow:
 *
 * ```ts
 * new Proxy({}, { get: () => () => new Promise(() => {}) })
 * ```
 *
 * That answers `api.tasks` with a function, so `api.tasks.list` is `undefined`
 * and calling it throws. Nine unhandled rejections came from that one file, and
 * Vitest is right that they "might cause false positive tests" — a component
 * that threw on mount can still leave an empty state an assertion is happy
 * with.
 *
 * So: one implementation, recursive, shared.
 */
export function idleApi() {
  const idle: unknown = new Proxy(function () {} as object, {
    // `then` must be undefined, or awaiting the proxy would recurse forever:
    // anything with a `then` is a thenable, and this one would hand back
    // itself.
    get: (_target, key) => (key === 'then' ? undefined : idle),
    apply: () => new Promise(() => {}),
  })
  return idle
}

/** What a `vi.mock('./api/client', …)` factory should return. */
export function idleClient() {
  return { api: idleApi(), connectEvents: () => () => {} }
}
