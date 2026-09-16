import { describe, expect, it } from 'vitest'
import { DEFAULT_READER_LAYOUT, readerLayoutFrom } from './reader-layout'
import { useStore } from '../state/store'

describe('reader layout preferences', () => {
  it('defaults to a reading surface instead of multiple open inspectors', () => {
    expect(readerLayoutFrom(null)).toEqual(DEFAULT_READER_LAYOUT)
    expect(DEFAULT_READER_LAYOUT.notesOpen).toBe(false)
    expect(DEFAULT_READER_LAYOUT.detailsOpen).toBe(false)
  })

  it('restores bounded widths and ignores malformed saved values', () => {
    expect(readerLayoutFrom({ rail: -20, notes: 9999, navigation: false, notesOpen: true }))
      .toMatchObject({ rail: 132, notes: 460, navigation: false, notesOpen: true })
    expect(readerLayoutFrom({ rail: NaN, notes: 'wide' })).toEqual(DEFAULT_READER_LAYOUT)
  })

  it('restores independent navigation and note pane sizes from settings', () => {
    useStore.getState().restorePrefs({
      'ui.readerLayout': JSON.stringify({ rail: 320, notes: 360, navigation: false, notesOpen: true }),
    })

    expect(useStore.getState().readerLayout)
      .toMatchObject({ rail: 320, notes: 360, navigation: false, notesOpen: true })
  })

  it('remembers a hidden library sidebar without losing its saved width', () => {
    useStore.getState().restorePrefs({
      'ui.sidebarOpen': 'false',
      'ui.layout': JSON.stringify({ sidebar: 290, detail: 380 }),
    })
    expect(useStore.getState().sidebarOpen).toBe(false)
    expect(useStore.getState().layout.sidebar).toBe(290)
  })
})
