import { useT } from '../i18n'
import { useStore } from '../state/store'
import { withToast } from '../ui'
import { CollectionEditor } from './CollectionEditor'

/**
 * Renders the collection editor for whatever the store points at.
 *
 * Both kinds go through here because they are one dialog: which kind a new
 * collection becomes is decided inside it, not by which button was pressed.
 */
export function CollectionEditorHost({ target }: { target?: string }) {
  const t = useT()
  const editing = target === 'new' ? null : target
  const collections = useStore((s) => s.collections)
  const smartCollections = useStore((s) => s.smartCollections)
  const saveCollection = useStore((s) => s.saveCollection)
  const closeTab = useStore((s) => s.closeTab)

  const plain = collections.find((c) => c.key === editing)
  const smart = smartCollections.find((c) => c.key === editing)
  const existing = plain ?? smart
  // Closing the editor closes its tab: the surface *is* the editor, so
  // leaving an empty one behind would be a tab about nothing.
  const close = () => closeTab(target ? `collection-edit:${target}` : 'collection-edit')

  return (
    <CollectionEditor
      title={existing ? t('collection.edit') : t('collection.new')}
      lockKind={!!existing}
      initial={
        existing && {
          name: existing.name,
          smart: !!smart,
          query: smart?.query ?? '',
          color: existing.color,
          icon: existing.icon,
        }
      }
      onCancel={close}
      onSubmit={async (values) => {
        await withToast(() => saveCollection(existing?.key ?? null, values), {
          success: existing ? t('toast.saved') : t('toast.created', { name: values.name }),
          failure: t('toast.createFailed'),
        })
        close()
      }}
    />
  )
}
