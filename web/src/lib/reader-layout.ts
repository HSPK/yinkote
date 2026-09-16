export interface ReaderLayout {
  navigation: boolean
  notesOpen: boolean
  detailsOpen: boolean
  rail: number
  notes: number
}

export const DEFAULT_READER_LAYOUT: ReaderLayout = {
  navigation: true,
  notesOpen: false,
  detailsOpen: false,
  rail: 200,
  notes: 280,
}

export function readerLayoutFrom(value: unknown): ReaderLayout {
  const saved = value && typeof value === 'object' ? value : {}
  const size = (value: unknown, fallback: number, min: number, max: number) =>
    typeof value === 'number' && Number.isFinite(value)
      ? Math.min(max, Math.max(min, value))
      : fallback
  return {
    navigation: 'navigation' in saved && typeof saved.navigation === 'boolean'
      ? saved.navigation : DEFAULT_READER_LAYOUT.navigation,
    notesOpen: 'notesOpen' in saved && typeof saved.notesOpen === 'boolean'
      ? saved.notesOpen : DEFAULT_READER_LAYOUT.notesOpen,
    detailsOpen: 'detailsOpen' in saved && typeof saved.detailsOpen === 'boolean'
      ? saved.detailsOpen : DEFAULT_READER_LAYOUT.detailsOpen,
    rail: size('rail' in saved ? saved.rail : null, DEFAULT_READER_LAYOUT.rail, 132, 420),
    notes: size('notes' in saved ? saved.notes : null, DEFAULT_READER_LAYOUT.notes, 220, 460),
  }
}
