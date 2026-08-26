import { useEffect, useMemo, useRef, useState } from 'react'

import { rankMatches } from '../lib/fuzzy'
import { PAGES, PAGE_LABELS, navigate } from '../lib/router'
import { useStore } from '../state/store'
import { confirmAction, promptFor, withToast } from '../ui'

interface Command {
  id: string
  label: string
  hint?: string
  run: () => void | Promise<void>
}

export function CommandPalette() {
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
        label: '新建条目…',
        hint: 'N',
        run: async () => {
          const values = await useStore.getState().newItemDialog()
          if (values) {
            await withToast(() => store.createItem(values.itemType, values.title), {
              success: `已创建「${values.title}」`,
              failure: '创建条目失败',
            })
          }
        },
      },
      {
        id: 'new-collection',
        label: '新建收藏夹…',
        run: async () => {
          const name = await promptFor('新建收藏夹', { label: '名称' })
          if (name) {
            await withToast(() => store.createCollection(name), {
              success: `已创建「${name}」`,
              failure: '创建收藏夹失败',
            })
          }
        },
      },
      { id: 'library', label: '打开：我的文库', run: store.openLibrary },
      { id: 'trash', label: '打开：回收站', run: store.openTrash },
      ...PAGES.map((id) => ({
        id: `page-${id}`,
        label: `前往：${PAGE_LABELS[id].label}`,
        run: () => navigate(id),
      })),
      { id: 'clear', label: '清除筛选与搜索', run: store.clearFilters },
      { id: 'reindex', label: '重建搜索索引', run: store.reindex },
      { id: 'reload-plugins', label: '重新扫描插件', run: store.reloadPlugins },
    ]
    if (store.selected.length) {
      list.unshift({
        id: 'trash-selected',
        label: `移入回收站（${store.selected.length} 条）`,
        hint: 'Del',
        run: store.trashSelected,
      })
      if (store.view === 'trash') {
        list.unshift(
          { id: 'restore', label: `还原（${store.selected.length} 条）`, run: store.restoreSelected },
          {
            id: 'destroy',
            label: `永久删除（${store.selected.length} 条）`,
            run: async () => {
              const ok = await confirmAction(`永久删除 ${store.selected.length} 条？`, {
                description: '此操作不可撤销，条目及其笔记、附件都会被移除。',
                confirmLabel: '永久删除',
                danger: true,
              })
              if (ok) {
                await withToast(store.destroySelected, {
                  success: '已永久删除',
                  failure: '删除失败',
                })
              }
            },
          },
        )
      }
      for (const c of store.collections) {
        list.push({
          id: `add-${c.key}`,
          label: `加入收藏夹：${c.name}`,
          run: () => store.addSelectedToCollection(c.key),
        })
      }
    }
    for (const m of ['hybrid', 'keyword', 'fuzzy', 'semantic'] as const) {
      list.push({ id: `mode-${m}`, label: `搜索模式：${m}`, run: () => store.setMode(m) })
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
          placeholder="输入命令…"
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
          {visible.length === 0 && <div className="empty">无匹配命令</div>}
        </div>
      </div>
    </div>
  )
}
