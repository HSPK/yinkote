import { describe, expect, it } from 'vitest'

import { conversationMenu } from './menus'
import type { MenuItem } from '../ui'

/**
 * Every menu item must be able to do something.
 *
 * `MenuItem` calls `onSelect`, and an item carrying a handler under any other
 * name is a row that highlights, closes the menu and does nothing. That is
 * exactly what shipped: the sidebar's conversation menu spelled it `run`, so
 * "Rename…" and "Delete" were dead for a fortnight without anything failing —
 * the property was excess on a union type, which TypeScript let through.
 */
function handlerless(items: MenuItem[]): string[] {
  return items
    .filter((i) => i.label !== undefined && !i.onSelect && !i.items)
    .map((i) => i.label!)
}

describe('context menus', () => {
  it('gives every conversation action something to do', () => {
    const menu = conversationMenu({
      key: 'ABCD1234',
      libraryId: 1,
      title: 'A thread',
      messageCount: 3,
      createdAt: 0,
      updatedAt: 0,
    })
    expect(menu.length).toBeGreaterThan(0)
    expect(handlerless(menu)).toEqual([])
  })
})
