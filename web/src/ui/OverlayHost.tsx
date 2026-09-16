/** Renders whatever the overlay store currently holds.
 *
 *  Mounted once at the app root; nothing else needs to know these exist.
 */
import { useEffect, useLayoutEffect, useRef, useState } from 'react'

import { t } from '../i18n'
import { Button } from './controls'
import { useOverlays, type MenuItem, type MenuSource } from './overlays'

// ─── dialog ─────────────────────────────────────────────────────────────────

function DialogHost() {
  const dialog = useOverlays((s) => s.dialog)
  const resolve = useOverlays((s) => s.resolveDialog)
  const formRef = useRef<HTMLFormElement>(null)
  const [values, setValues] = useState<Record<string, string>>({})

  useEffect(() => {
    if (!dialog) return
    setValues(
      Object.fromEntries(
        (dialog.fields ?? []).map((f) => [f.name, f.defaultValue ?? f.options?.[0]?.value ?? '']),
      ),
    )
    // Focus after paint, otherwise the caret has nowhere to land.
    const id = requestAnimationFrame(() => {
      const target = formRef.current?.querySelector<HTMLElement>('[data-autofocus]')
      target?.focus()
      if (target instanceof HTMLInputElement) target.select()
    })
    return () => cancelAnimationFrame(id)
  }, [dialog])

  if (!dialog) return null

  const missing = (dialog.fields ?? []).some((f) => f.required && !values[f.name]?.trim())

  const submit = (e: React.FormEvent) => {
    e.preventDefault()
    if (missing) return
    resolve(values)
  }

  return (
    <div className="overlay" onMouseDown={() => resolve(null)}>
      <form
        ref={formRef}
        className="dialog"
        onMouseDown={(e) => e.stopPropagation()}
        onSubmit={submit}
        onKeyDown={(e) => {
          if (e.key === 'Escape') resolve(null)
          // Ctrl/Cmd+Enter submits from inside a textarea.
          if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) submit(e)
        }}
      >
        <header className="dialog-head">{dialog.title}</header>
        {dialog.description && <p className="dialog-desc">{dialog.description}</p>}

        {dialog.fields && dialog.fields.length > 0 && (
          <div className="dialog-body">
            {dialog.fields.map((field) => (
              <label className="field" key={field.name}>
                <span className="field-label">{field.label}</span>
                {field.type === 'textarea' ? (
                  <textarea
                    className="ctl"
                    rows={4}
                    spellCheck={false}
                    data-autofocus={field.autoFocus || undefined}
                    placeholder={field.placeholder}
                    value={values[field.name] ?? ''}
                    onChange={(e) => setValues({ ...values, [field.name]: e.target.value })}
                  />
                ) : field.type === 'select' ? (
                  <select
                    className="ctl"
                    data-autofocus={field.autoFocus || undefined}
                    value={values[field.name] ?? ''}
                    onChange={(e) => setValues({ ...values, [field.name]: e.target.value })}
                  >
                    {(field.options ?? []).map((o) => (
                      <option key={o.value} value={o.value}>
                        {o.label}
                      </option>
                    ))}
                  </select>
                ) : (
                  <input
                    className="ctl"
                    spellCheck={false}
                    autoComplete="off"
                    data-autofocus={field.autoFocus || undefined}
                    placeholder={field.placeholder}
                    value={values[field.name] ?? ''}
                    onChange={(e) => setValues({ ...values, [field.name]: e.target.value })}
                  />
                )}
                {field.hint && <span className="field-hint">{field.hint}</span>}
              </label>
            ))}
          </div>
        )}

        <footer className="dialog-foot">
          <Button type="button" tone="ghost" onClick={() => resolve(null)}>
            {dialog.cancelLabel ?? t('dialog.cancel')}
          </Button>
          <Button type="submit" tone={dialog.danger ? 'danger' : 'primary'} disabled={missing}>
            {dialog.confirmLabel ?? t('dialog.confirm')}
          </Button>
        </footer>
      </form>
    </div>
  )
}

// ─── context menu ───────────────────────────────────────────────────────────

function MenuRow({ item, onDone }: { item: MenuItem; onDone: () => void }) {
  const [open, setOpen] = useState(false)
  const anchor = useRef<HTMLDivElement>(null)
  const submenu = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ left: 0, top: 0 })

  useLayoutEffect(() => {
    if (!open || !anchor.current || !submenu.current) return
    const rect = anchor.current.getBoundingClientRect()
    const child = submenu.current.getBoundingClientRect()
    setPosition({
      left: Math.max(8, rect.right + child.width > window.innerWidth - 8
        ? rect.left - child.width : rect.right),
      top: Math.max(8, Math.min(rect.top, window.innerHeight - child.height - 8)),
    })
  }, [open, item.items])

  if (!item.label) return <div className="menu-sep" role="separator" />

  if (item.items?.length) {
    return (
      <div
        ref={anchor}
        className="menu-item"
        role="menuitem"
        aria-label={item.label}
        tabIndex={item.disabled ? -1 : 0}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-disabled={item.disabled || undefined}
        data-disabled={item.disabled || undefined}
        onMouseEnter={() => !item.disabled && setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onClick={() => !item.disabled && setOpen(true)}
        onKeyDown={(e) => {
          if (item.disabled || e.target !== e.currentTarget) return
          if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowRight') {
            e.preventDefault()
            setOpen(true)
            requestAnimationFrame(() => submenu.current?.querySelector<HTMLElement>('[role^="menuitem"]')?.focus())
          }
          if (e.key === 'ArrowLeft') setOpen(false)
        }}
      >
        <span>{item.label}</span>
        <span className="menu-hint">›</span>
        {open && (
          <div ref={submenu} className="menu submenu" role="menu"
            style={{ position: 'fixed', ...position, maxHeight: 'calc(100vh - 16px)', overflowY: 'auto' }}
            onKeyDown={(event) => {
              if (event.key !== 'ArrowLeft') return
              event.preventDefault()
              event.stopPropagation()
              setOpen(false)
              anchor.current?.focus()
            }}
          >
            {item.items.map((child, i) => (
              <MenuRow key={child.id ?? i} item={child} onDone={onDone} />
            ))}
          </div>
        )}
      </div>
    )
  }

  const checkable = item.checked !== undefined
  return (
    <button
      className="menu-item"
      role={checkable ? 'menuitemcheckbox' : 'menuitem'}
      aria-label={item.label}
      aria-checked={checkable ? item.checked : undefined}
      data-danger={item.danger || undefined}
      data-checked={item.checked || undefined}
      disabled={item.disabled}
      onClick={() => {
        // Toggles stay open: choosing columns is nearly always several choices,
        // and reopening the menu between each is needless work.
        if (!checkable) onDone()
        void item.onSelect?.()
        if (checkable) useOverlays.getState().refreshMenu()
      }}
    >
      {checkable && <span className="menu-check" aria-hidden="true">{item.checked ? '✓' : ''}</span>}
      <span>{item.label}</span>
      {item.hint && <span className="menu-hint">{item.hint}</span>}
    </button>
  )
}

function MenuHost() {
  const menu = useOverlays((s) => s.menu)
  const close = useOverlays((s) => s.closeMenu)
  const ref = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ x: 0, y: 0 })
  const menuId = menu?.id

  useEffect(() => {
    if (menuId === undefined) return
    const previous = document.activeElement as HTMLElement | null
    const frame = requestAnimationFrame(() =>
      ref.current?.querySelector<HTMLElement>('[role^="menuitem"]:not(:disabled):not([aria-disabled="true"])')?.focus(),
    )
    return () => {
      cancelAnimationFrame(frame)
      if (previous?.isConnected) previous.focus()
    }
  }, [menuId])

  // Flip the menu when it would run off screen; measure before paint so the
  // user never sees it jump.
  useLayoutEffect(() => {
    if (!menu || !ref.current) return
    const rect = ref.current.getBoundingClientRect()
    setPosition({
      x: Math.max(8, Math.min(menu.x, window.innerWidth - rect.width - 8)),
      y: Math.max(8, Math.min(menu.y, window.innerHeight - rect.height - 8)),
    })
  }, [menu])

  useEffect(() => {
    if (!menu) return
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && close()
    window.addEventListener('keydown', onKey)
    window.addEventListener('resize', close)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('resize', close)
    }
  }, [menu, close])

  if (!menu) return null

  return (
    <div className="menu-backdrop" onMouseDown={close} onContextMenu={(e) => e.preventDefault()}>
      <div
        ref={ref}
        className="menu"
        role="menu"
        style={{ left: position.x, top: position.y }}
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(event) => {
          if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
          const current = event.target as HTMLElement
          const parent = current.closest('[role="menu"]')
          const rows = [...(parent?.querySelectorAll<HTMLElement>(
            ':scope > [role^="menuitem"]:not(:disabled):not([aria-disabled="true"])',
          ) ?? [])]
          if (!rows.length) return
          event.preventDefault()
          const index = rows.indexOf(current)
          const next = event.key === 'Home' ? 0 : event.key === 'End' ? rows.length - 1
            : (index + (event.key === 'ArrowDown' ? 1 : -1) + rows.length) % rows.length
          rows[next]?.focus()
        }}
      >
        {menu.items.map((item, i) => (
          <MenuRow key={item.id ?? i} item={item} onDone={close} />
        ))}
      </div>
    </div>
  )
}

// ─── toasts ─────────────────────────────────────────────────────────────────

function ToastHost() {
  const toasts = useOverlays((s) => s.toasts)
  const dismiss = useOverlays((s) => s.dismissToast)

  if (!toasts.length) return null
  return (
    <div className="toasts">
      {toasts.map((entry) => (
        <div key={entry.id} className="toast" data-tone={entry.tone}>
          <div className="toast-body">
            <span className="toast-message">{entry.message}</span>
            {entry.detail && <span className="toast-detail">{entry.detail}</span>}
          </div>
          {entry.action && (
            <button
              className="toast-action"
              onClick={() => {
                dismiss(entry.id)
                entry.action?.onSelect()
              }}
            >
              {entry.action.label}
            </button>
          )}
          <button className="toast-close" onClick={() => dismiss(entry.id)} title={t('toast.dismiss')}>
            ×
          </button>
        </div>
      ))}
    </div>
  )
}

export function OverlayHost() {
  return (
    <>
      <DialogHost />
      <MenuHost />
      <ToastHost />
    </>
  )
}

/** Attach to `onContextMenu`. Returns a handler that opens `items`. */
export function contextMenu(items: MenuSource) {
  return (event: React.MouseEvent) => {
    event.preventDefault()
    event.stopPropagation()
    useOverlays.getState().openMenu(event.clientX, event.clientY, items)
  }
}
