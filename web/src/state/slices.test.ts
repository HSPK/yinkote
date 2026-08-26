import { readFileSync, readdirSync } from 'node:fs'
import { join } from 'node:path'
import { describe, expect, it } from 'vitest'

const DIR = join('src', 'state', 'slices')
const slices = readdirSync(DIR).filter((f) => f.endsWith('.ts') && !f.includes('.test.'))

describe('store slices', () => {
  it('has slices to check', () => {
    expect(slices.length).toBeGreaterThan(0)
  })

  it('imports the composed State as a type only', () => {
    // The slice pattern is inherently circular: the store composes the slices
    // and each slice is typed against the whole. That is harmless while the
    // import is erased at compile time, and a runtime cycle the moment it is
    // not — one slice would evaluate to `undefined` with no error anywhere.
    for (const file of slices) {
      const source = readFileSync(join(DIR, file), 'utf8')
      const storeImports = source
        .split('\n')
        .filter((line) => /from '\.\.\/store'/.test(line))

      expect(storeImports, file).not.toEqual([])
      for (const line of storeImports) {
        expect(line.trim(), `${file}: must be a type-only import`).toMatch(/^import type /)
      }
    }
  })

  it('declares its own initial values, so the store cannot be half-built', () => {
    for (const file of slices) {
      const source = readFileSync(join(DIR, file), 'utf8')
      expect(source, file).toMatch(/export const create\w+Slice: StateCreator</)
    }
  })
})
