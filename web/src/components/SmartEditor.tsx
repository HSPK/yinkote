import { useMemo, useState } from 'react'

import { useSchemaLabel, useT } from '../i18n'
import {
  OPERATORS,
  queryFromRules,
  rulesFromQuery,
  type Field,
  type Operator,
  type Rule,
} from '../lib/query'
import { useStore } from '../state/store'
import { Button, Field as Row, Icon, Input, Modal, Select } from '../ui'

const FIELDS: Field[] = ['text', 'tag', 'type', 'author', 'year']

export interface SmartEditorProps {
  title: string
  initial?: { name: string; query: string }
  onCancel: () => void
  onSubmit: (values: { name: string; query: string }) => void | Promise<void>
}

/**
 * Builds a smart collection from Field / Operator / Value rows.
 *
 * The rows compile to the ordinary query language rather than to a private rule
 * format, so a smart collection is always exactly what the search box would
 * have found — and the compiled query is shown, because a saved search that
 * cannot be read is a saved search nobody trusts.
 */
export function SmartEditor({ title, initial, onCancel, onSubmit }: SmartEditorProps) {
  const t = useT()
  const label = useSchemaLabel()
  const schema = useStore((s) => s.schema)
  const tags = useStore((s) => s.tags)

  const [name, setName] = useState(initial?.name ?? '')
  const [rules, setRules] = useState<Rule[]>(() => {
    const parsed = rulesFromQuery(initial?.query ?? '')
    return parsed.length ? parsed : [{ field: 'text', op: 'contains', value: '' }]
  })
  const [busy, setBusy] = useState(false)

  const query = useMemo(() => queryFromRules(rules), [rules])

  const patch = (index: number, change: Partial<Rule>) =>
    setRules((current) =>
      current.map((rule, i) => (i === index ? { ...rule, ...change } : rule)),
    )

  const changeField = (index: number, field: Field) =>
    // The operator must stay valid for the new field, so reset it rather than
    // leaving an "is not" on a field that has no negation.
    patch(index, { field, op: OPERATORS[field][0]!, value: '', value2: undefined })

  const typeOptions = (schema?.itemTypes ?? [])
    .filter((d) => !d.internal)
    .map((d) => ({ value: d.type, label: label(d, d.type) }))

  const submit = async () => {
    if (!name.trim() || busy) return
    setBusy(true)
    try {
      await onSubmit({ name: name.trim(), query })
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal title={title} width="narrow" onClose={onCancel}>
      <div className="page narrow rule-editor">
        <Row label={t('dialog.name')}>
          <Input value={name} autoFocus onChange={(e) => setName(e.target.value)} />
        </Row>

        <Row label={t('smart.rules')} hint={t('smart.rulesHint')}>
          <div className="rules">
            {rules.map((rule, i) => (
              <div className="rule" key={i}>
                <Select
                  value={rule.field}
                  options={FIELDS.map((f) => ({ value: f, label: t(`search.field.${f}`) }))}
                  onChange={(e) => changeField(i, e.target.value as Field)}
                />
                <Select
                  value={rule.op}
                  options={OPERATORS[rule.field].map((op) => ({
                    value: op,
                    label: t(`smart.op.${op}`),
                  }))}
                  onChange={(e) => patch(i, { op: e.target.value as Operator })}
                />

                {rule.field === 'type' ? (
                  <Select
                    value={rule.value}
                    options={[{ value: '', label: '—' }, ...typeOptions]}
                    onChange={(e) => patch(i, { value: e.target.value })}
                  />
                ) : (
                  <Input
                    value={rule.value}
                    list={rule.field === 'tag' ? 'smart-tags' : undefined}
                    placeholder={t('smart.value')}
                    onChange={(e) => patch(i, { value: e.target.value })}
                  />
                )}

                {rule.op === 'between' && (
                  <Input
                    value={rule.value2 ?? ''}
                    placeholder={t('smart.value2')}
                    onChange={(e) => patch(i, { value2: e.target.value })}
                  />
                )}

                <button
                  className="icon-btn"
                  title={t('smart.removeRule')}
                  disabled={rules.length === 1}
                  onClick={() => setRules(rules.filter((_, j) => j !== i))}
                >
                  <Icon.Close size={11} />
                </button>
              </div>
            ))}

            <datalist id="smart-tags">
              {tags.map((tag) => (
                <option key={tag.name} value={tag.name} />
              ))}
            </datalist>

            <Button
              tone="ghost"
              onClick={() => setRules([...rules, { field: 'tag', op: 'is', value: '' }])}
            >
              {t('smart.addRule')}
            </Button>
          </div>
        </Row>

        <Row label={t('smart.compiled')} hint={t('smart.compiledHint')}>
          <code className="code compiled">{query || t('smart.matchesEverything')}</code>
        </Row>

        <footer className="dialog-foot">
          <Button tone="ghost" onClick={onCancel}>
            {t('dialog.cancel')}
          </Button>
          <Button tone="primary" disabled={!name.trim() || busy} onClick={() => void submit()}>
            {initial ? t('dialog.save') : t('dialog.create')}
          </Button>
        </footer>
      </div>
    </Modal>
  )
}
