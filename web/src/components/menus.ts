/** Context-menu definitions.
 *
 *  Kept out of the components so the same menu can be raised from a row, the
 *  detail panel or the command palette without duplicating the actions — and so
 *  the wording and ordering stay consistent.
 */
import type { Collection, Item } from '../api/types'
import { useStore } from '../state/store'
import { confirmAction, promptFor, toast, withToast, type MenuItem } from '../ui'

export function itemMenu(item: Item): MenuItem[] {
  const store = useStore.getState()
  const selected = store.selected.includes(item.key) ? store.selected : [item.key]
  const many = selected.length > 1
  const suffix = many ? `（${selected.length} 条）` : ''

  // Right-clicking an unselected row should act on that row.
  if (!store.selected.includes(item.key)) store.select(item.key)

  const copy = (kind: 'title' | 'doi' | 'url' | 'citation', label: string) => ({
    label,
    onSelect: async () => {
      const n = await store.copySelected(kind)
      if (n === 0) toast.info(`选中的条目没有${label.replace('复制', '')}`)
      else toast.success(`已复制 ${n} 条`)
    },
  })

  const inTrash = store.view === 'trash'

  return [
    {
      label: '在详情中打开',
      onSelect: () => {
        store.select(item.key)
        store.setPanel('detail')
      },
    },
    ...(item.url || item.DOI
      ? [
          {
            label: '在浏览器中打开',
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
    copy('title', '复制标题'),
    copy('citation', '复制引用'),
    copy('doi', '复制 DOI'),
    {},
    {
      label: `加入收藏夹${suffix}`,
      disabled: store.collections.length === 0,
      items: store.collections.map((c) => ({
        label: c.name,
        onSelect: () =>
          withToast(() => store.addSelectedToCollection(c.key), {
            success: `已加入「${c.name}」`,
            failure: '加入收藏夹失败',
          }),
      })),
    },
    {
      label: `添加标签…${suffix}`,
      onSelect: async () => {
        const tag = await promptFor('添加标签', {
          label: '标签',
          placeholder: '例如：综述',
        })
        if (tag) {
          await withToast(() => store.tagSelected(tag), {
            success: `已添加标签「${tag}」`,
            failure: '添加标签失败',
          })
        }
      },
    },
    {},
    ...(inTrash
      ? [
          {
            label: `还原${suffix}`,
            onSelect: () =>
              withToast(store.restoreSelected, { success: '已还原', failure: '还原失败' }),
          },
          {
            label: `永久删除${suffix}`,
            danger: true,
            onSelect: async () => {
              const ok = await confirmAction(`永久删除 ${selected.length} 条？`, {
                description: '此操作不可撤销。',
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
        ]
      : [
          {
            label: `移入回收站${suffix}`,
            hint: 'Del',
            danger: true,
            onSelect: () =>
              withToast(store.trashSelected, {
                success: `已移入回收站${suffix}`,
                failure: '移入回收站失败',
              }),
          },
        ]),
  ]
}

export function collectionMenu(collection: Collection): MenuItem[] {
  const store = useStore.getState()
  return [
    { label: '打开', onSelect: () => store.openCollection(collection.key) },
    {},
    {
      label: '重命名…',
      onSelect: async () => {
        const name = await promptFor('重命名收藏夹', {
          label: '名称',
          defaultValue: collection.name,
        })
        if (name && name !== collection.name) {
          await withToast(() => store.renameCollection(collection.key, name), {
            success: '已重命名',
            failure: '重命名失败',
          })
        }
      },
    },
    {
      label: '新建子收藏夹…',
      onSelect: async () => {
        const name = await promptFor('新建子收藏夹', {
          label: '名称',
          hint: `将建立在「${collection.name}」下`,
        })
        if (name) {
          await withToast(() => store.createCollection(name, collection.key), {
            success: `已创建「${name}」`,
            failure: '创建失败',
          })
        }
      },
    },
    {},
    {
      label: '删除收藏夹',
      danger: true,
      onSelect: async () => {
        const ok = await confirmAction(`删除「${collection.name}」？`, {
          description: '收藏夹内的条目会保留在文库中，子收藏夹会上移一层。',
          confirmLabel: '删除',
          danger: true,
        })
        if (ok) {
          await withToast(() => store.removeCollection(collection.key), {
            success: '已删除收藏夹',
            failure: '删除失败',
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
      label: '新建条目…',
      onSelect: async () => {
        const values = await store.newItemDialog()
        if (values) {
          await withToast(() => store.createItem(values.itemType, values.title), {
            success: `已创建「${values.title}」`,
            failure: '创建失败',
          })
        }
      },
    },
    {
      label: '新建收藏夹…',
      onSelect: async () => {
        const name = await promptFor('新建收藏夹', { label: '名称' })
        if (name) {
          await withToast(() => store.createCollection(name), {
            success: `已创建「${name}」`,
            failure: '创建失败',
          })
        }
      },
    },
    {},
    { label: '清除筛选与搜索', onSelect: store.clearFilters },
    { label: '重建搜索索引', onSelect: () => withToast(store.reindex, { success: '索引已重建', failure: '重建失败' }) },
  ]
}

export function trashMenu(): MenuItem[] {
  const store = useStore.getState()
  return [
    { label: '打开回收站', onSelect: store.openTrash },
    {},
    {
      label: '清空回收站',
      danger: true,
      onSelect: async () => {
        const ok = await confirmAction('清空回收站？', {
          description: '回收站中的所有条目都会被永久删除。',
          confirmLabel: '清空',
          danger: true,
        })
        if (ok) {
          await withToast(store.emptyTrash, { success: '回收站已清空', failure: '清空失败' })
        }
      },
    },
  ]
}
