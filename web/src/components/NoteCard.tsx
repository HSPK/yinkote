import type { Item } from '../api/types'
import { useT } from '../i18n'
import { shortDate } from '../lib/format'
import { notePreview } from '../lib/note-preview'

export function NoteCard({ note, onOpen }: { note: Item; onOpen: () => void }) {
  const t = useT()
  const preview = notePreview(note)
  const title = typeof note.title === 'string' ? note.title.trim() : ''
  return (
    <button className="note-card" data-note={note.key}
      data-colour="blue"
      title={t('reader.openNote')} onClick={onOpen}>
      <span className="note-page note-card-meta">
        <span>{t('note.title')}</span>
        <time dateTime={note.dateModified ? new Date(note.dateModified).toISOString() : undefined}>
          {shortDate(note.dateModified)}
        </time>
      </span>
      {title && <span className="note-card-title">{title}</span>}
      <span className="note-text">{preview || t('reader.blankNote')}</span>
    </button>
  )
}
