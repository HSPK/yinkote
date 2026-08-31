import { useMemo, useState } from "react";

import { useSchemaLabel, useT } from "../i18n";
import {
  OPERATORS,
  queryFromRules,
  rulesFromQuery,
  type Field,
  type Operator,
  type Rule,
} from "../lib/query";
import {
  COLLECTION_COLOURS,
  COLLECTION_ICONS,
  collectionIcon,
} from "../lib/collections";
import { useStore } from "../state/store";
import {
  Button,
  Field as Row,
  Icon,
  Input,
  Select,
  Toggle,
} from "../ui";

const FIELDS: Field[] = ["text", "tag", "type", "author", "year"];

export interface CollectionValues {
  name: string;
  smart: boolean;
  query: string;
  color?: string;
  icon?: string;
}

export interface SmartEditorProps {
  initial?: Partial<CollectionValues>;
  /** A saved collection cannot change kind: its contents mean different things. */
  lockKind?: boolean;
  onCancel: () => void;
  onSubmit: (values: CollectionValues) => void | Promise<void>;
}

/**
 * One dialog for both kinds of collection.
 *
 * "Smart" is a property of a collection, not a separate thing to create, so it
 * is a switch here rather than a second button in the sidebar. Turning it on
 * reveals Field / Operator / Value rows, which compile to the ordinary query
 * language rather than to a private rule format — and the compiled query is
 * shown, because a saved search nobody can read is a saved search nobody
 * trusts.
 */
export function CollectionEditor({
  initial,
  lockKind,
  onCancel,
  onSubmit,
}: SmartEditorProps) {
  const t = useT();
  const label = useSchemaLabel();
  const schema = useStore((s) => s.schema);
  const tags = useStore((s) => s.tags);

  const [name, setName] = useState(initial?.name ?? "");
  const [smart, setSmart] = useState(initial?.smart ?? false);
  const [colour, setColour] = useState(initial?.color ?? "");
  const [icon, setIcon] = useState(initial?.icon ?? "");
  const [rules, setRules] = useState<Rule[]>(() => {
    const parsed = rulesFromQuery(initial?.query ?? "");
    return parsed.length
      ? parsed
      : [{ field: "text", op: "contains", value: "" }];
  });
  const [busy, setBusy] = useState(false);

  const query = useMemo(() => queryFromRules(rules), [rules]);

  const patch = (index: number, change: Partial<Rule>) =>
    setRules((current) =>
      current.map((rule, i) => (i === index ? { ...rule, ...change } : rule)),
    );

  const changeField = (index: number, field: Field) =>
    // The operator must stay valid for the new field, so reset it rather than
    // leaving an "is not" on a field that has no negation.
    patch(index, {
      field,
      op: OPERATORS[field][0]!,
      value: "",
      value2: undefined,
    });

  const typeOptions = (schema?.itemTypes ?? [])
    .filter((d) => !d.internal)
    .map((d) => ({ value: d.type, label: label(d, d.type) }));

  const submit = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    try {
      await onSubmit({
        name: name.trim(),
        smart,
        query: smart ? query : "",
        color: colour || undefined,
        icon: icon || undefined,
      });
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="pane main surface">
      {/* No heading: the tab is already called "Edit collection", and the
          surface saying it again is the same words twice at the top of a
          column that has none to spare. */}
      <div className="page narrow rule-editor">
        <Row label={t("dialog.name")}>
          <Input
            value={name}
            autoFocus
            onChange={(e) => setName(e.target.value)}
          />
        </Row>

        <Row
          label={t("collection.appearance")}
          hint={t("collection.appearanceHint")}
        >
          <div className="appearance">
            <div className="swatches">
              <button
                className="swatch"
                data-active={!colour}
                title={t("collection.noColour")}
                onClick={() => setColour("")}
              />
              {COLLECTION_COLOURS.map((c) => (
                <button
                  key={c}
                  className="swatch"
                  data-colour={c}
                  data-active={colour === c}
                  title={t(`collection.colour.${c}`)}
                  onClick={() => setColour(c)}
                />
              ))}
            </div>
            <div className="icon-picks">
              {COLLECTION_ICONS.map((name) => {
                const Glyph = collectionIcon(name);
                return (
                  <button
                    key={name}
                    className="icon-btn"
                    data-active={icon === name || (!icon && name === "folder")}
                    title={t(`collection.icon.${name}`)}
                    onClick={() => setIcon(name)}
                  >
                    <Glyph />
                  </button>
                );
              })}
            </div>
          </div>
        </Row>

        <Row label={t("collection.smart")} hint={t("collection.smartHint")}>
          <Toggle checked={smart} disabled={lockKind} onChange={setSmart} />
        </Row>

        {smart && (
          <>
            <Row label={t("smart.rules")} hint={t("smart.rulesHint")}>
              <div className="rules">
                {rules.map((rule, i) => (
                  <div className="rule" key={i}>
                    <Select
                      value={rule.field}
                      options={FIELDS.map((f) => ({
                        value: f,
                        label: t(`search.field.${f}`),
                      }))}
                      onChange={(e) => changeField(i, e.target.value as Field)}
                    />
                    <Select
                      value={rule.op}
                      options={OPERATORS[rule.field].map((op) => ({
                        value: op,
                        label: t(`smart.op.${op}`),
                      }))}
                      onChange={(e) =>
                        patch(i, { op: e.target.value as Operator })
                      }
                    />

                    {rule.field === "type" ? (
                      <Select
                        value={rule.value}
                        options={[{ value: "", label: "—" }, ...typeOptions]}
                        onChange={(e) => patch(i, { value: e.target.value })}
                      />
                    ) : (
                      <Input
                        value={rule.value}
                        list={rule.field === "tag" ? "smart-tags" : undefined}
                        placeholder={t("smart.value")}
                        onChange={(e) => patch(i, { value: e.target.value })}
                      />
                    )}

                    {rule.op === "between" && (
                      <Input
                        value={rule.value2 ?? ""}
                        placeholder={t("smart.value2")}
                        onChange={(e) => patch(i, { value2: e.target.value })}
                      />
                    )}

                    <button
                      className="icon-btn"
                      title={t("smart.removeRule")}
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
                  onClick={() =>
                    setRules([...rules, { field: "tag", op: "is", value: "" }])
                  }
                >
                  {t("smart.addRule")}
                </Button>
              </div>
            </Row>

            <Row label={t("smart.compiled")} hint={t("smart.compiledHint")}>
              <code className="code compiled">
                {query || t("smart.matchesEverything")}
              </code>
            </Row>
          </>
        )}

        <footer className="dialog-foot">
          <Button tone="ghost" onClick={onCancel}>
            {t("dialog.cancel")}
          </Button>
          <Button
            tone="primary"
            disabled={!name.trim() || busy}
            onClick={() => void submit()}
          >
            {initial ? t("dialog.save") : t("dialog.create")}
          </Button>
        </footer>
      </div>
    </div>
  );
}
