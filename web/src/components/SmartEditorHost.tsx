import { useT } from '../i18n'
import { useStore } from '../state/store'
import { withToast } from '../ui'
import { SmartEditor } from './SmartEditor'

/** Renders the rule editor for whichever smart collection the store points at. */
export function SmartEditorHost() {
  const t = useT()
  const editing = useStore((s) => s.smartEditor)
  const smartCollections = useStore((s) => s.smartCollections)
  const openSmartEditor = useStore((s) => s.openSmartEditor)
  const createSmart = useStore((s) => s.createSmart)
  const updateSmart = useStore((s) => s.updateSmart)

  if (!editing) return null

  const existing = smartCollections.find((s) => s.key === editing)
  const close = () => openSmartEditor(null)

  return (
    <SmartEditor
      title={existing ? t('dialog.editSmart') : t('dialog.newSmart')}
      initial={existing ? { name: existing.name, query: existing.query } : undefined}
      onCancel={close}
      onSubmit={async (values) => {
        await withToast(
          () =>
            existing
              ? updateSmart(existing.key, values)
              : createSmart(values.name, values.query),
          {
            success: existing ? t('toast.saved') : t('toast.created', { name: values.name }),
            failure: t('toast.createFailed'),
          },
        )
        close()
      }}
    />
  )
}
