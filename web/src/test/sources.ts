import { readdirSync, readFileSync } from 'node:fs'
import { join } from 'node:path'

export interface Source {
  path: string
  text: string
  lines: string[]
}

/**
 * Every source file, for the tests that police the tree rather than a function.
 *
 * Three of them walk `src` — the catalogue checks, the client-method check and
 * the markup check — and each had grown its own copy of this. Callers filter:
 * what counts as "not my own file" differs for each, and getting that wrong is
 * how an audit ends up passing itself (see docs/16 3.172).
 *
 * Test files are excluded: they are allowed to contain the very things the
 * rules forbid, because that is how a rule is proved to work.
 */
export function sourceFiles(): Source[] {
  const walk = (dir: string): string[] =>
    readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
      const path = join(dir, entry.name)
      if (entry.isDirectory()) return walk(path)
      return /\.tsx?$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name) ? [path] : []
    })

  return walk('src').map((path) => {
    const text = readFileSync(path, 'utf8')
    return { path, text, lines: text.split('\n') }
  })
}
