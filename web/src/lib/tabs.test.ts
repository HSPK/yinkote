import { describe, expect, it } from 'vitest'

import {
  LIBRARY_TAB_ID,
  closeAll,
  closeOthers,
  closeTab,
  libraryTab,
  nextActive,
  openTab,
  tabId,
  type Tab,
} from './tabs'

const library = libraryTab('Library')
const chat = { id: tabId('chat', 'C1'), kind: 'chat', title: 'Ask', target: 'C1' } as Tab
const reader = { id: tabId('reader', 'I1'), kind: 'reader', title: 'Paper', target: 'I1' } as Tab

describe('tabs', () => {
  it('gives one id per thing, so a second request focuses the first tab', () => {
    expect(tabId('reader', 'I1')).toBe(tabId('reader', 'I1'))
    expect(tabId('reader', 'I1')).not.toBe(tabId('reader', 'I2'))
    expect(tabId('plugins')).toBe('plugins')
  })

  it('opens a tab once', () => {
    const once = openTab([library], chat)
    expect(openTab(once, chat)).toBe(once)
    expect(once).toHaveLength(2)
  })

  it('returns the same array when nothing changed, so React can skip a render', () => {
    const tabs = openTab([library], chat)
    expect(openTab(tabs, chat)).toBe(tabs)
  })

  it('refreshes a title, so a renamed conversation is not stale in the bar', () => {
    const tabs = openTab([library, chat], { ...chat, title: 'Renamed' })
    expect(tabs[1]?.title).toBe('Renamed')
  })

  it('never closes the workbench itself', () => {
    expect(closeTab([library], LIBRARY_TAB_ID)).toHaveLength(1)
    expect(closeAll([library, chat, reader])).toEqual([library])
    expect(closeOthers([library, chat, reader], reader.id)).toEqual([library, reader])
  })

  it('closes an ordinary tab', () => {
    expect(closeTab([library, chat], chat.id)).toEqual([library])
  })

  it('activates the neighbour to the right, then the left', () => {
    // Closing a run left to right keeps the hand in one place.
    const tabs = [library, chat, reader]
    expect(nextActive(tabs, chat.id, chat.id)).toBe(reader.id)
    expect(nextActive(tabs, reader.id, reader.id)).toBe(chat.id)
  })

  it('leaves the active tab alone when a different one closes', () => {
    expect(nextActive([library, chat, reader], chat.id, reader.id)).toBe(reader.id)
  })

  it('falls back to the library when the last tab closes', () => {
    expect(nextActive([chat], chat.id, chat.id)).toBe(LIBRARY_TAB_ID)
  })
})
