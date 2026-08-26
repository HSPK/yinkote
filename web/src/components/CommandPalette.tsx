import { useEffect, useMemo, useRef, useState } from 'react'

import { rankMatches } from '../lib/fuzzy'
import { useStore } from '../state/store'
import { confirmAction, promptFor, withToast } from '../ui'
import { useT } from '../i18n'

interface Command {
  id: string
  label: string
  hint?: string
  run: () => void | Promise<void>
}

export function CommandPalette() {
  const t = useT()
  const open = useStore((s) => s.paletteOpen)
  const toggle = useStore((s) => s.togglePalette)
  const store = useStore()
  const [query, setQuery] = useState('')
  const [cursor, setCursor] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)

  const commands = useMemo<Command[]>(() => {
    const list: Command[] = [
      {
        id: 'new',
        label: t('menu.newItem'),
        hint: 'N',
        run: async () => {
          const values = await useStore.getState().newItemDialog()
          if (values) {
            await withToast(() => store.createItem(values.itemType, values.title), {
              success: t('toast.created', { name: values.title }),
              failure: t('toast.createFailed'),
            })
          }
        },
      },
      {
        id: 'new-collection',
        label: t('menu.newCollection'),
        run: async () => {
          const name = await promptFor(t('dialog.newCollection'), { label: t('dialog.name') })
          if (name) {
            await withToast(() => store.createCollection(name), {
              success: t('toast.created', { name }),
              failure: t('toast.createFailed'),
            })
          }
        },
      },
      
      { id: 'trash', label: t('menu.openTrash'), run: store.openTrash },
      ...(['plugins', 'status', 'settings'] as const).map((id) => ({
        id: `modal-${id}`,
        label: t('palette.goto', { page: t(`nav.${id}`) }),
        run: () => store.setModal(id),
      })),
      { id: 'new-chat', label: t('chat.new'), run: store.newConversation },
      { id: 'clear', label: t('menu.clearFilters'), run: store.clearFilters },
      { id: 'reindex', label: t('menu.reindex'), run: store.reindex },
      { id: 'reload-plugins', label: t('plugins.rescan'), run: store.reloadPlugins },
    ]
    if (store.selected.length) {
      list.unshift({
        id: 'trash-selected',
        label: `${t('menu.trash')}${t('menu.selection', { count: store.selected.length })}`,
        hint: 'Del',
        run: store.trashSelected,
      })
      if (store.view === 'trash') {
        list.unshift(
          {
            id: 'restore',
            label: `${t('menu.restore')}${t('menu.selection', { count: store.selected.length })}`,
            run: store.restoreSelected,
          },
          {
            id: 'destroy',
            label: `${t('menu.destroy')}${t('menu.selection', { count: store.selected.length })}`,
            run: async () => {
              const ok = await confirmAction(
                t('dialog.destroyTitle', { count: store.selected.length }),
                {
                  description: t('dialog.destroyDesc'),
                  confirmLabel: t('menu.destroy'),
                  cancelLabel: t('dialog.cancel'),
                  danger: true,
                },
              )
              if (ok) {
                await withToast(store.destroySelected, {
                  success: t('toast.destroyed'),
                  failure: t('toast.deleteFailed'),
                })
              }
            },
          },
        )
      }
      for (const c of store.collections) {
        list.push({
          id: `add-${c.key}`,
          label: t('palette.addTo', { name: c.name }),
          run: () => store.addSelectedToCollection(c.key),
        })
      }
    }
    return list
  }, [store])

  const visible = useMemo(
    () => rankMatches(query, commands, (c) => c.label),
    [commands, query],
  )

  useEffect(() => {
    if (open) {
      setQuery('')
      setCursor(0)
      // Focus after the overlay paints, otherwise the caret lands nowhere.
      requestAnimationFrame(() => inputRef.current?.focus())
    }
  }, [open])

  if (!open) return null

  const runAt = (index: number) => {
    const cmd = visible[index]
    if (!cmd) return
    toggle(false)
    void cmd.run()
  }

  return (
    <div className="overlay" onMouseDown={() => toggle(false)}>
      <div className="palette" onMouseDown={(e) => e.stopPropagation()}>
        <input
          ref={inputRef}
          value={query}
          placeholder={t('palette.placeholder')}
          spellCheck={false}
          onChange={(e) => {
            setQuery(e.target.value)
            setCursor(0)
          }}
          onKeyDown={(e) => {
            if (e.key === 'Escape') toggle(false)
            else if (e.key === 'ArrowDown') {
              e.preventDefault()
              setCursor((c) => Math.min(visible.length - 1, c + 1))
            } else if (e.key === 'ArrowUp') {
              e.preventDefault()
              setCursor((c) => Math.max(0, c - 1))
            } else if (e.key === 'Enter') {
              e.preventDefault()
              runAt(cursor)
            }
          }}
        />
        <div className="palette-list">
          {visible.map((c, i) => (
            <button
              key={c.id}
              className="palette-item"
              data-active={i === cursor}
              onMouseEnter={() => setCursor(i)}
              onClick={() => runAt(i)}
            >
              <span>{c.label}</span>
              {c.hint && <span className="hint">{c.hint}</span>}
            </button>
          ))}
          {visible.length === 0 && <div className="empty">{t('palette.noMatch')}</div>}
        </div>
      </div>
    </div>
  )
}
