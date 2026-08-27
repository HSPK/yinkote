/** Context-menu definitions.
 *
 *  Kept out of the components so the same menu can be raised from a row, the
 *  detail panel or the command palette without duplicating the actions — and so
 *  the wording and ordering stay consistent.
 */
import { api } from '../api/client'
import type { Collection, Item, SmartCollection, Tag } from '../api/types'
import { hasChosenColour, TAG_COLOURS } from '../lib/tags'
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
    { label: t('graph.open'), onSelect: () => store.openGraph(item.key) },
    {
      // Plural because a paper often has a PDF, a supplement and a dataset, and
      // asking three times is three chances to give up.
      label: t('menu.download'),
      onSelect: async () => {
        const text = await promptFor(t('menu.download'), {
          label: t('dialog.urls'),
          type: 'textarea',
          defaultValue: String(item.url ?? ''),
        })
        const urls = (text ?? '').split('\n').map((u) => u.trim()).filter(Boolean)
        if (!urls.length) return
        await withToast(
          async () => {
            const done = await api.downloads.enqueue(store.library, item.key, urls)
            toast.success(t('downloads.queued', { count: done.queued }))
          },
          { failure: t('downloads.actionFailed') },
        )
      },
    },
    ...(item.DOI
      ? [
          {
            label: t('references.fetch'),
            onSelect: () =>
              withToast(async () => {
                const got = await api.references.fetch(store.library, item.key)
                if (!got.stored) throw new Error(t('references.none'))
                toast.success(t('references.fetched', got))
              }, { failure: t('references.failed') }),
          },
        ]
      : []),
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
          pending: t('summary.working'),
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

/**
 * What a tag can do.
 *
 * Colour first because it is the only thing here that is not destructive, and
 * because a tag's colour is how a library becomes readable at a glance.
 */
export function tagMenu(tag: Tag): MenuItem[] {
  const store = useStore.getState()

  return [
    {
      label: t('tag.colour'),
      items: TAG_COLOURS.map((colour) => ({
        label: `${t(`colour.${colour}`)}${tag.color === colour ? ' \u2713' : ''}`,
        onSelect: () =>
          withToast(() => store.setTagColour(tag.name, colour), {
            failure: t('tag.colourFailed'),
          }),
      })),
    },
    {
      // Clearing is not "no colour": the tag goes back to the one derived from
      // its name, which is what it wore before anybody chose.
      label: t('tag.colourAuto'),
      disabled: !hasChosenColour(tag.color),
      onSelect: () =>
        withToast(() => store.setTagColour(tag.name, ''), { failure: t('tag.colourFailed') }),
    },
    {},
    {
      label: t('tag.rename'),
      onSelect: async () => {
        const next = await promptFor(t('tag.rename'), { label: t('dialog.tag'), defaultValue: tag.name })
        if (next && next !== tag.name) {
          await withToast(() => store.renameTag(tag.name, next), {
            success: t('toast.saved'),
            failure: t('tag.renameFailed'),
          })
        }
      },
    },
    {
      label: t('tag.delete'),
      danger: true,
      onSelect: async () => {
        if (await confirmAction(t('tag.deleteConfirm', { name: tag.name }))) {
          await withToast(() => store.deleteTag(tag.name), {
            success: t('toast.saved'),
            failure: t('tag.deleteFailed'),
          })
        }
      },
    },
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
