/** The commands the workbench can perform.
 *
 *  Defined once because they have two front doors — the command palette and the
 *  context menus — and a command written twice drifts. It already had: creating
 *  a collection existed as both a bare name prompt and the full editor, so
 *  which one you got depended on where you clicked.
 *
 *  An action knows its own label and what it does. Neither surface knows how
 *  any of them work, and adding one means adding it here.
 */
import { t, type MessageKey } from '../i18n'
import { useStore } from '../state/store'
import { runOptimize, runReindex } from '../lib/maintenance'
import { confirmAction, withToast } from '../ui'

export interface Action {
  /** Stable identifier, used as a React key and for the palette's ranking. */
  id: string
  label: string
  /** Keyboard shortcut, shown but not bound here. */
  hint?: string
  danger?: boolean
  run: () => void | Promise<void>
}

function action(id: string, label: MessageKey, run: Action['run'], extra: Partial<Action> = {}) {
  return { id, label: t(label), run, ...extra }
}

/** Create an item, having asked what kind and what it is called. */
export function newItem(): Action {
  return action('new-item', 'menu.newItem', async () => {
    const store = useStore.getState()
    const values = await store.newItemDialog()
    if (!values) return
    await withToast(() => store.createItem(values.itemType, values.title), {
      success: t('toast.created', { name: values.title }),
      failure: t('toast.createFailed'),
    })
  }, { hint: 'N' })
}

/** Create a collection, plain or smart, through the full editor. */
export function newCollection(): Action {
  return action('new-collection', 'collection.new', () =>
    useStore.getState().openCollectionEditor('new'),
  )
}

/** Permanently delete the selection, after asking. */
export function destroySelected(count: number): Action {
  return action(
    'destroy',
    'menu.destroy',
    async () => {
      const store = useStore.getState()
      const ok = await confirmAction(t('dialog.destroyTitle', { count }), {
        description: t('dialog.destroyDesc'),
        confirmLabel: t('menu.destroy'),
        cancelLabel: t('dialog.cancel'),
        danger: true,
      })
      if (!ok) return
      await withToast(store.destroySelected, {
        success: t('toast.destroyed'),
        failure: t('toast.deleteFailed'),
      })
    },
    { danger: true },
  )
}

export function reindex(): Action {
  return action('reindex', 'menu.reindex', runReindex)
}

export function optimize(): Action {
  return action('optimize', 'statusPage.optimize', runOptimize)
}

export function clearFilters(): Action {
  return action('clear-filters', 'menu.clearFilters', () => useStore.getState().clearFilters())
}

export function openTrash(): Action {
  return action('trash', 'menu.openTrash', () => useStore.getState().openTrash())
}

export function reloadPlugins(): Action {
  return action('reload-plugins', 'plugins.rescan', () => useStore.getState().reloadPlugins())
}

/** Everything offered without a selection or a right-click target. */
export function globalActions(): Action[] {
  return [
    newItem(),
    newCollection(),
    openTrash(),
    clearFilters(),
    reindex(),
    optimize(),
    reloadPlugins(),
  ]
}

/** An action rendered as a context-menu entry. */
export function asMenuItem(a: Action) {
  return { label: a.label, hint: a.hint, danger: a.danger, onSelect: a.run }
}
