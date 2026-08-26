/** Imperative overlays: dialogs, context menus and toasts.
 *
 *  These are the three things that are painful to express declaratively — you
 *  want to *ask* a question at a call site and await the answer, not thread
 *  open/close state through five components. A single store plus one host
 *  component at the root gives call sites the ergonomics of `window.prompt`
 *  with a real interface behind it.
 */
import { create } from 'zustand'

import { t } from '../i18n'

// ─── dialogs ────────────────────────────────────────────────────────────────

export interface DialogField {
  name: string
  label: string
  /** `text` is a single line, `textarea` is multi-line, `select` needs options. */
  type?: 'text' | 'textarea' | 'select'
  placeholder?: string
  defaultValue?: string
  options?: { value: string; label: string }[]
  required?: boolean
  autoFocus?: boolean
  hint?: string
}

export interface DialogSpec {
  title: string
  description?: string
  fields?: DialogField[]
  confirmLabel?: string
  cancelLabel?: string
  /** Renders the confirm button as destructive. */
  danger?: boolean
}

export type DialogResult = Record<string, string> | null

interface PendingDialog extends DialogSpec {
  id: number
  resolve: (result: DialogResult) => void
}

// ─── context menu ───────────────────────────────────────────────────────────

export interface MenuItem {
  /** A separator when omitted. */
  label?: string
  hint?: string
  danger?: boolean
  disabled?: boolean
  /** Renders a tick and keeps the menu open, for toggles like column visibility. */
  checked?: boolean
  onSelect?: () => void | Promise<void>
  /** Nested submenu, one level deep. */
  items?: MenuItem[]
}

interface OpenMenu {
  x: number
  y: number
  items: MenuItem[]
}

// ─── toasts ─────────────────────────────────────────────────────────────────

export type ToastTone = 'info' | 'success' | 'error'

export interface Toast {
  id: number
  tone: ToastTone
  message: string
  detail?: string
  /** Optional single action, e.g. "Undo". */
  action?: { label: string; onSelect: () => void }
}

interface OverlayState {
  dialog: PendingDialog | null
  menu: OpenMenu | null
  toasts: Toast[]

  ask: (spec: DialogSpec) => Promise<DialogResult>
  resolveDialog: (result: DialogResult) => void
  openMenu: (x: number, y: number, items: MenuItem[]) => void
  closeMenu: () => void
  pushToast: (toast: Omit<Toast, 'id'>) => number
  dismissToast: (id: number) => void
}

let sequence = 0
const nextId = () => ++sequence

/** How long a toast stays up. Errors linger; they usually need reading. */
const TOAST_MS: Record<ToastTone, number> = { info: 3200, success: 2600, error: 7000 }

export const useOverlays = create<OverlayState>((set, get) => ({
  dialog: null,
  menu: null,
  toasts: [],

  ask(spec) {
    // Only one dialog at a time: a stack of modals is a design smell.
    const existing = get().dialog
    if (existing) existing.resolve(null)
    return new Promise<DialogResult>((resolve) => {
      set({ dialog: { ...spec, id: nextId(), resolve } })
    })
  },

  resolveDialog(result) {
    const dialog = get().dialog
    if (!dialog) return
    set({ dialog: null })
    dialog.resolve(result)
  },

  openMenu(x, y, items) {
    set({ menu: items.length ? { x, y, items } : null })
  },

  closeMenu() {
    set({ menu: null })
  },

  pushToast(toast) {
    const id = nextId()
    set({ toasts: [...get().toasts, { ...toast, id }] })
    // Plain `setTimeout`, not `window.setTimeout`: this module has no other
    // reason to require a DOM, and staying environment-agnostic keeps it
    // directly testable.
    setTimeout(() => get().dismissToast(id), TOAST_MS[toast.tone])
    return id
  },

  dismissToast(id) {
    set({ toasts: get().toasts.filter((t) => t.id !== id) })
  },
}))

// ─── call-site helpers ──────────────────────────────────────────────────────

/** Ask for a single value. Resolves to `null` when cancelled. */
export async function promptFor(
  title: string,
  field: Omit<DialogField, 'name'>,
  spec: Partial<DialogSpec> = {},
): Promise<string | null> {
  const result = await useOverlays.getState().ask({
    title,
    ...spec,
    fields: [{ name: 'value', autoFocus: true, required: true, ...field }],
  })
  return result?.value?.trim() ?? null
}

/** Ask a yes/no question. */
export async function confirmAction(
  title: string,
  spec: Partial<DialogSpec> = {},
): Promise<boolean> {
  const result = await useOverlays.getState().ask({
    title,
    confirmLabel: t('dialog.confirm'),
    cancelLabel: t('dialog.cancel'),
    ...spec,
  })
  return result !== null
}

export const toast = {
  info: (message: string, detail?: string) =>
    useOverlays.getState().pushToast({ tone: 'info', message, detail }),
  success: (message: string, detail?: string) =>
    useOverlays.getState().pushToast({ tone: 'success', message, detail }),
  error: (message: string, detail?: string) =>
    useOverlays.getState().pushToast({ tone: 'error', message, detail }),
  /** Report a caught error without every call site formatting it. */
  fromError: (message: string, error: unknown) =>
    useOverlays.getState().pushToast({
      tone: 'error',
      message,
      detail: error instanceof Error ? error.message : String(error),
    }),
}

/** Wrap an async action so failures surface as a toast instead of vanishing. */
export async function withToast<T>(
  action: () => Promise<T>,
  messages: { success?: string; failure: string },
): Promise<T | undefined> {
  try {
    const result = await action()
    if (messages.success) toast.success(messages.success)
    return result
  } catch (error) {
    toast.fromError(messages.failure, error)
    return undefined
  }
}
