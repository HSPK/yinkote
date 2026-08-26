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

export function itemMenu(item: Item): MenuItem[] {
  const store = useStore.getState()
  const selected = store.selected.includes(item.key) ? store.selected : [item.key]
  const many = selected.length > 1
  const suffix = many ? t('menu.selection', { count: selected.length }) : ''

  // Right-clicking an unselected row should act on that row.
  if (!store.selected.includes(item.key)) store.select(item.key)

  const copy = (kind: 'title' | 'doi' | 'url' | 'citation', key: 'menu.copyTitle' | 'menu.copyCitation' | 'menu.copyDoi') => ({
    label: t(key),
    onSelect: async () => {
      const n = await store.copySelected(kind)
      if (n === 0) toast.info(t('toast.nothingToCopy'))
      else toast.success(t('toast.copied', { count: n }))
    },
  })

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
    copy('citation', 'menu.copyCitation'),
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
            label: `${t('menu.destroy')}${suffix}`,
            danger: true,
            onSelect: async () => {
              const ok = await confirmAction(
                t('dialog.destroyTitle', { count: selected.length }),
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

export function newCollection(): void {
  useStore.getState().openCollectionEditor('new')
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
  const store = useStore.getState()
  return [
    {
      label: t('menu.newItem'),
      onSelect: async () => {
        const values = await store.newItemDialog()
        if (values) {
          await withToast(() => store.createItem(values.itemType, values.title), {
            success: t('toast.created', { name: values.title }),
            failure: t('toast.createFailed'),
          })
        }
      },
    },
    {
      label: t('menu.newCollection'),
      onSelect: async () => {
        const name = await promptFor(t('dialog.newCollection'), { label: t('dialog.name') })
        if (name) {
          await withToast(() => store.createCollection(name), {
            success: t('toast.created', { name }),
            failure: t('toast.createFailed'),
          })
        }
      },
    },
    { label: t('collection.new'), onSelect: newCollection },
    {},
    { label: t('menu.clearFilters'), onSelect: store.clearFilters },
    {
      label: t('menu.reindex'),
      onSelect: () =>
        withToast(store.reindex, {
          success: t('toast.reindexed'),
          failure: t('toast.reindexFailed'),
        }),
    },
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
