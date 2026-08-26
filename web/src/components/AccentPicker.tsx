import { useEffect, useRef, useState } from 'react'

import { useT } from '../i18n'
import { isHexColour } from '../lib/theme'
import { Button, Icon } from '../ui'

/** A spread of accents that read well against every bundled theme. */
const PRESETS = [
  '#4da3ff',
  '#5ec8f2',
  '#5aa469',
  '#c9a227',
  '#d3584e',
  '#9b72d0',
  '#c96a9b',
  '#8a94a6',
]

export interface AccentPickerProps {
  value: string
  onChange: (accent: string) => void
}

/**
 * Accent chooser.
 *
 * A popover rather than the browser's colour dialog: that dialog cannot be
 * dismissed by clicking away, is styled by the operating system, and offers a
 * full gamut when what is wanted is a handful of colours that suit the themes.
 * The native input is still here, one click further in, for anyone who wants
 * an exact value.
 */
export function AccentPicker({ value, onChange }: AccentPickerProps) {
  const t = useT()
  const [open, setOpen] = useState(false)
  const [draft, setDraft] = useState(value)
  const root = useRef<HTMLDivElement>(null)

  useEffect(() => setDraft(value), [value])

  // Dismiss on a click anywhere else, or on Escape — the two things every
  // popover is expected to do and the native dialog does neither of.
  useEffect(() => {
    if (!open) return
    const onDown = (e: MouseEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false)
    }
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        setOpen(false)
      }
    }
    document.addEventListener('mousedown', onDown)
    document.addEventListener('keydown', onKey, true)
    return () => {
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('keydown', onKey, true)
    }
  }, [open])

  const commit = (accent: string) => {
    onChange(accent)
    setOpen(false)
  }

  return (
    <div className="accent-row" ref={root}>
      <button
        className="accent-button"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        title={t('settings.accent')}
      >
        <span
          className="accent-preview"
          style={{ background: isHexColour(value) ? value : 'var(--accent)' }}
        />
        <code>{value || t('settings.accentDefault')}</code>
        <Icon.ChevronDown size={11} />
      </button>

      {value && (
        <Button tone="ghost" onClick={() => commit('')}>
          {t('settings.reset')}
        </Button>
      )}

      {open && (
        <div className="accent-pop">
          <div className="accent-grid">
            {PRESETS.map((preset) => (
              <button
                key={preset}
                className="swatch"
                style={{ background: preset }}
                data-active={value.toLowerCase() === preset}
                title={preset}
                onClick={() => commit(preset)}
              />
            ))}
          </div>

          <div className="accent-custom">
            <input
              type="color"
              className="accent-picker"
              value={isHexColour(draft) ? draft : '#4da3ff'}
              onChange={(e) => setDraft(e.target.value)}
            />
            <input
              className="ctl"
              value={draft}
              spellCheck={false}
              placeholder="#4da3ff"
              onChange={(e) => setDraft(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && isHexColour(draft) && commit(draft)}
            />
            <Button tone="primary" disabled={!isHexColour(draft)} onClick={() => commit(draft)}>
              {t('dialog.confirm')}
            </Button>
          </div>
        </div>
      )}
    </div>
  )
}
