/** Context-menu definitions.
 *
 *  Kept out of the components so the same menu can be raised from a row, the
 *  detail panel or the command palette without duplicating the actions — and so
 *  the wording and ordering stay consistent.
 */
import type { Collection, Item, SmartCollection } from '../api/types'
import { t } from '../i18n'
import { useStore } from '../state/store'
import { confirmAction, promptFor, toast, withToast, type MenuItem } from '../ui'
import {
  asMenuItem,
  clearFilters,
  destroySelected,
  newCollection,
  newItem,
  reindex,
} from './actions'

export function itemMenu(item: Item): MenuItem[] {
  const store = useStore.getState()
  const selected = store.selected.includes(item.key) ? store.selected : [item.key]
  const many = selected.length > 1
  const suffix = many ? t('menu.selection', { count: selected.length }) : ''

  // Right-clicking an unselected row should act on that row.
  if (!store.selected.includes(item.key)) store.select(item.key)

  const copied = (n: number) => {
    if (n === 0) toast.info(t('toast.nothingToCopy'))
    else toast.success(t('toast.copied', { count: n }))
  }

  const copy = (kind: 'title' | 'doi' | 'url', key: 'menu.copyTitle' | 'menu.copyDoi') => ({
    label: t(key),
    onSelect: async () => copied(await store.copySelected(kind)),
  })

  // Every style, rather than one buried in settings: which style is wanted
  // depends on where the reference is going, and that changes per paste.
  const copyCitation = {
    label: `${t('menu.copyCitation')}${suffix}`,
    items: store.citationStyles.map((style) => ({
      label: style.id === store.citationStyle ? `${style.name} \u2713` : style.name,
      onSelect: async () =>
        withToast(async () => copied(await store.copySelected('citation', style.id)), {
          failure: t('toast.citationFailed'),
        }),
    })),
    disabled: store.citationStyles.length === 0,
  }

  const inTrash = store.view === 'trash'

  return [
    { label: t('reader.open'), onSelect: () => store.openReader(item.key) },
    {
      label: t('reader.fetch'),
      onSelect: () =>
        withToast(() => store.fetchPdf(item.key), {
          success: t('reader.fetched', { name: String(item.title ?? item.key) }),
          failure: t('reader.fetchFailed'),
        }),
    },
    {
      label: t('chat.askAbout'),
      onSelect: () =>
        withToast(() => store.askAbout(item.key), { failure: t('summary.failed') }),
    },
    {
      label: t('summary.generate'),
      onSelect: () =>
        withToast(() => store.summarise(item.key), {
          success: t('summary.done'),
          failure: t('summary.failed'),
        }),
    },
    {},
    {
      label: t('menu.openDetail'),
      onSelect: () => {
        store.select(item.key)
        store.setPanel('detail')
      },
    },
    ...(item.url || item.DOI
      ? [
          {
            label: t('menu.openBrowser'),
            onSelect: () => {
              const href = item.url
                ? String(item.url)
                : `https://doi.org/${String(item.DOI)}`
              window.open(href, '_blank', 'noopener')
            },
          },
        ]
      : []),
    {},
    copy('title', 'menu.copyTitle'),
    copyCitation,
    copy('doi', 'menu.copyDoi'),
    {},
    {
      label: `${t('menu.addToCollection')}${suffix}`,
      disabled: store.collections.length === 0,
      items: store.collections.map((c) => ({
        label: c.name,
        onSelect: () =>
          withToast(() => store.addSelectedToCollection(c.key), {
            success: t('toast.addedToCollection', { name: c.name }),
            failure: t('toast.addToCollectionFailed'),
          }),
      })),
    },
    {
      label: `${t('menu.addTag')}${suffix}`,
      onSelect: async () => {
        const tag = await promptFor(t('dialog.addTag'), { label: t('dialog.tag') })
        if (tag) {
          await withToast(() => store.tagSelected(tag), {
            success: t('toast.tagAdded', { tag }),
            failure: t('toast.tagFailed'),
          })
        }
      },
    },
    {},
    ...(inTrash
      ? [
          {
            label: `${t('menu.restore')}${suffix}`,
            onSelect: () =>
              withToast(store.restoreSelected, {
                success: t('toast.restored'),
                failure: t('toast.restoreFailed'),
              }),
          },
          {
            ...asMenuItem(destroySelected(selected.length)),
            label: `${t('menu.destroy')}${suffix}`,
          },
        ]
      : [
          {
            label: `${t('menu.trash')}${suffix}`,
            hint: 'Del',
            danger: true,
            onSelect: () =>
              withToast(store.trashSelected, {
                success: t('toast.trashed'),
                failure: t('toast.trashFailed'),
              }),
          },
        ]),
  ]
}

export function collectionMenu(collection: Collection): MenuItem[] {
  const store = useStore.getState()
  return [
    { label: t('menu.open'), onSelect: () => store.openCollection(collection.key) },
    {},
    { label: t('menu.edit'), onSelect: () => store.openCollectionEditor(collection.key) },
    {
      label: t('menu.newSubcollection'),
      onSelect: async () => {
        const name = await promptFor(t('dialog.newSubcollection'), {
          label: t('dialog.name'),
          hint: t('dialog.underCollection', { name: collection.name }),
        })
        if (name) {
          await withToast(() => store.createCollection(name, collection.key), {
            success: t('toast.created', { name }),
            failure: t('toast.createFailed'),
          })
        }
      },
    },
    {},
    {
      label: t('menu.deleteCollection'),
      danger: true,
      onSelect: async () => {
        const ok = await confirmAction(
          t('dialog.deleteCollectionTitle', { name: collection.name }),
          {
            description: t('dialog.deleteCollectionDesc'),
            confirmLabel: t('dialog.delete'),
            cancelLabel: t('dialog.cancel'),
            danger: true,
          },
        )
        if (ok) {
          await withToast(() => store.removeCollection(collection.key), {
            success: t('toast.deleted'),
            failure: t('toast.deleteFailed'),
          })
        }
      },
    },
  ]
}

export function smartMenu(smart: SmartCollection): MenuItem[] {
  const store = useStore.getState()
  return [
    { label: t('menu.open'), onSelect: () => store.openSmart(smart.key) },
    {},
    { label: t('menu.editSmart'), onSelect: () => store.openCollectionEditor(smart.key) },
    {},
    {
      label: t('menu.deleteSmart'),
      danger: true,
      onSelect: async () => {
        const ok = await confirmAction(t('dialog.deleteSmartTitle', { name: smart.name }), {
          description: t('dialog.deleteSmartDesc'),
          confirmLabel: t('dialog.delete'),
          cancelLabel: t('dialog.cancel'),
          danger: true,
        })
        if (ok) {
          await withToast(() => store.removeSmart(smart.key), {
            success: t('toast.deleted'),
            failure: t('toast.deleteFailed'),
          })
        }
      },
    },
  ]
}

export function libraryMenu(): MenuItem[] {
  return [
    asMenuItem(newItem()),
    asMenuItem(newCollection()),
    {},
    asMenuItem(clearFilters()),
    asMenuItem(reindex()),
  ]
}

export function trashMenu(): MenuItem[] {
  const store = useStore.getState()
  return [
    { label: t('menu.openTrash'), onSelect: store.openTrash },
    {},
    {
      label: t('menu.emptyTrash'),
      danger: true,
      onSelect: async () => {
        const ok = await confirmAction(t('dialog.emptyTrashTitle'), {
          description: t('dialog.emptyTrashDesc'),
          confirmLabel: t('menu.emptyTrash'),
          cancelLabel: t('dialog.cancel'),
          danger: true,
        })
        if (ok) {
          await withToast(store.emptyTrash, {
            success: t('toast.emptied'),
            failure: t('toast.emptyFailed'),
          })
        }
      },
    },
  ]
}
