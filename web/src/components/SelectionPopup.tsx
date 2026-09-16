import { useT } from '../i18n'
import { HIGHLIGHT_COLOURS, type HighlightColour, type Mark } from '../lib/annotations'
import { Icon } from '../ui'
import type { ScreenRect } from '../lib/selection-toolbar'
import { FloatingTools } from './FloatingTools'

const NO_RECTS: ScreenRect[] = []

/**
 * What to do with the text somebody just selected.
 *
 * This exists because the reader used to answer that question for them:
 * releasing the mouse over a selection wrote a highlight into the library.
 * Selecting a sentence to copy it therefore left an annotation behind, and
 * there was no way to read with the mouse without editing.
 *
 * So the selection is now a *question*. Nothing is written until one of these
 * is clicked, and dismissing costs a click anywhere or the Escape key.
 */
export function SelectionPopup({
  at,
  colour,
  onMark,
  onCopy,
  onCite,
  onDismiss,
  selection = NO_RECTS,
}: {
  /** Where the selection ended, in viewport coordinates. */
  at: { x: number; y: number }
  colour: HighlightColour
  onMark: (kind: Mark, colour: HighlightColour) => void
  onCopy: () => void
  onCite: () => void
  onDismiss: () => void
  selection?: readonly ScreenRect[]
}) {
  const t = useT()

  return (
    <FloatingTools at={at} selection={selection} label={t('reader.selectionTools')} onDismiss={onDismiss}>
      <div className="swatches">
        {HIGHLIGHT_COLOURS.map((c) => (
          <button
            key={c}
            className="swatch"
            data-colour={c}
            data-active={c === colour}
            title={t('reader.highlight')}
            onClick={() => onMark('highlight', c)}
          />
        ))}
      </div>
      <button className="popup-action" title={t('reader.underline')} onClick={() => onMark('underline', colour)}>
        <span className="underline-glyph">A</span>
      </button>
      <button className="popup-action" title={t('reader.copyText')} onClick={onCopy}>
        <Icon.Copy size={12} />
      </button>
      <button className="popup-action" title={t('reader.copyCitation')} onClick={onCite}>
        <Icon.Library size={12} />
      </button>
    </FloatingTools>
  )
}
