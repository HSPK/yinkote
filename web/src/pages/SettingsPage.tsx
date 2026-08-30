import { useEffect, useMemo, useRef, useState } from 'react'
import { embedderMeaning } from '../lib/format'

import { api } from '../api/client'
import type { AgentStatus, SourceInfo } from '../api/types'
import { LOCALES, useI18n, useT, type Locale } from '../i18n'
import { AccentPicker } from '../components/AccentPicker'
import { ArchiveImport } from '../components/ArchiveImport'
import { BibliographyImport } from '../components/BibliographyImport'
import { BrowserConnector, LibraryAccess } from '../components/BrowserConnector'
import { WordAddin } from '../components/WordAddin'
import { ZoteroImport } from '../components/ZoteroImport'
import { filterSettings, type SettingSection } from '../lib/settings'
import { THEMES } from '../lib/theme'
import { useStore } from '../state/store'
import {
  runBackup,
  runExportAll,
  runIntegrity,
  runOptimize,
  runReindex,
} from '../lib/maintenance'
import { Badge, Button, Field, Icon, Input, Section, Select, toast } from '../ui'
import { copyText } from '../lib/clipboard'

const DENSITIES = ['compact', 'comfortable'] as const

export function SettingsPage() {
  const t = useT()
  const locale = useI18n((s) => s.locale)

  const server = useStore((s) => s.server)
  const refreshServer = useStore((s) => s.refreshServer)
  const stats = useStore((s) => s.stats)
  const mode = useStore((s) => s.mode)
  const density = useStore((s) => s.density)
  const citationStyle = useStore((s) => s.citationStyle)
  const citationStyles = useStore((s) => s.citationStyles)
  const setCitationStyle = useStore((s) => s.setCitationStyle)
  const theme = useStore((s) => s.theme)
  const accent = useStore((s) => s.accent)
  const setDensity = useStore((s) => s.setDensity)
  const setTheme = useStore((s) => s.setTheme)
  const setLocale = useStore((s) => s.setLocale)

  const agent = useStore((s) => s.agent)
  const configureAgent = useStore((s) => s.configureAgent)

  const [sources, setSources] = useState<SourceInfo[]>([])
  const [filter, setFilter] = useState('')
  const [current, setCurrent] = useState<string | null>(null)
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    api.scrape
      .sources()
      .then(setSources)
      .catch(() => setSources([]))
  }, [])

  const copy = async (value: string) => {
    await copyText(value)
    toast.success(t('toast.copiedPath'))
  }

  const sections: SettingSection[] = useMemo(
    () => [
      {
        id: 'appearance',
        title: t('settings.appearance'),
        fields: [
          {
            id: 'language',
            label: t('settings.language'),
            keywords: t('settings.keywords.language'),
            render: () => (
              <Select
                value={locale}
                options={LOCALES.map((l) => ({ value: l.value, label: l.label }))}
                onChange={(e) => setLocale(e.target.value as Locale)}
              />
            ),
          },
          {
            id: 'theme',
            label: t('settings.theme'),
            keywords: t('settings.keywords.theme'),
            render: () => (
              <div className="theme-grid">
                {THEMES.map((preset) => (
                  <button
                    key={preset.id}
                    className="theme-swatch"
                    data-active={theme === preset.id}
                    onClick={() => setTheme(preset.id)}
                    title={preset.name}
                  >
                    <span className="theme-preview">
                      {(['--bg', '--bg-2', '--fg-dim', '--accent'] as const).map((key) => (
                        <i key={key} style={{ background: preset.vars[key] }} />
                      ))}
                    </span>
                    <span className="theme-name">{preset.name}</span>
                  </button>
                ))}
              </div>
            ),
          },
          {
            id: 'accent',
            label: t('settings.accent'),
            keywords: t('settings.keywords.accent'),
            render: () => (
              <AccentPicker value={accent} onChange={(next) => setTheme(theme, next)} />
            ),
          },
          {
            id: 'citationStyle',
            label: t('settings.citationStyle'),
            hint: t('settings.citationStyleHint'),
            keywords: t('settings.keywords.citationStyle'),
            render: () => (
              <Select
                value={citationStyle}
                options={citationStyles.map((s) => ({ value: s.id, label: s.name }))}
                onChange={(e) => setCitationStyle(e.target.value)}
              />
            ),
          },
          {
            id: 'density',
            label: t('settings.density'),
            keywords: t('settings.keywords.density'),
            render: () => (
              <Select
                value={density}
                options={DENSITIES.map((d) => ({ value: d, label: t(`settings.density.${d}`) }))}
                onChange={(e) => setDensity(e.target.value)}
              />
            ),
          },
        ],
      },
      {
        id: 'model',
        title: t('settings.model'),
        fields: [
          {
            id: 'agent',
            label: t('settings.modelEndpoint'),
            hint: t('settings.modelHint'),
            keywords: t('settings.keywords.model'),
            render: () => <ModelSettings agent={agent} onSave={configureAgent} />,
          },
          {
            id: 'skills',
            label: t('settings.skills'),
            hint: t('settings.skillsHint'),
            keywords: t('settings.keywords.skills'),
            render: () => <SkillToggles agent={agent} onSave={configureAgent} />,
          },
          {
            id: 'tools',
            label: t('settings.tools'),
            hint: t('settings.toolsHint'),
            keywords: t('settings.keywords.tools'),
            render: () => <ToolToggles agent={agent} onSave={configureAgent} />,
          },
        ],
      },
      {
        id: 'search',
        title: t('settings.search'),
        fields: [
          {
            id: 'mode',
            label: t('settings.currentMode'),
            hint: t('settings.currentModeHint'),
            keywords: t('settings.keywords.mode'),
            render: () => <span className="ctl-static">{t(`search.mode.${mode}`)}</span>,
          },
          {
            id: 'syntax',
            label: t('settings.syntax'),
            hint: t('settings.syntaxHint'),
            keywords: t('settings.keywords.syntax'),
            render: () => (
              <div className="syntax">
                <code>tag:survey</code>
                <code>-tag:obsolete</code>
                <code>type:book</code>
                <code>author:zhang</code>
                <code>year:2020..2024</code>
                <code>&quot;exact phrase&quot;</code>
              </div>
            ),
          },
        ],
      },
      {
        id: 'quickAdd',
        title: t('settings.quickAdd'),
        fields: [
          {
            id: 'resolvers',
            label: t('settings.resolvers'),
            hint: t('settings.resolversHint'),
            keywords: t('settings.keywords.resolvers'),
            render: () => (
              <div className="chip-row tight">
                {sources.map((source) => (
                  <Badge key={source.id} tone="accent" title={source.supports.join(' / ')}>
                    {source.label}
                  </Badge>
                ))}
                {sources.length === 0 && <span className="muted">{t('settings.loading')}</span>}
              </div>
            ),
          },
        ],
      },
      {
        id: 'storage',
        title: t('settings.storage'),
        fields: [
          {
            id: 'dataDir',
            label: t('settings.dataDir'),
            hint: t('settings.dataDirHint'),
            keywords: t('settings.keywords.dataDir'),
            render: () => (
              <div className="path-row">
                <code>{server?.dataDir ?? '—'}</code>
                <Button
                  tone="ghost"
                  disabled={!server}
                  onClick={() => void copy(server?.dataDir ?? '')}
                >
                  {t('settings.copy')}
                </Button>
              </div>
            ),
          },
          {
            id: 'pluginDirs',
            label: t('settings.pluginDirs'),
            keywords: t('settings.keywords.pluginDirs'),
            render: () => (
              <ul className="path-list">
                {(server?.pluginDirs ?? []).map((dir) => (
                  <li key={dir}>{dir}</li>
                ))}
              </ul>
            ),
          },
        ],
      },
      {
        id: 'import',
        title: t('settings.import'),
        fields: [
          {
            id: 'zotero',
            label: t('import.zotero'),
            hint: t('import.zoteroHint'),
            keywords: t('settings.keywords.import'),
            render: () => <ZoteroImport />,
          },
          {
            id: 'bibliography',
            label: t('import.bib'),
            hint: t('import.bibHint'),
            keywords: t('settings.keywords.import'),
            render: () => <BibliographyImport />,
          },
          {
            id: 'archive',
            label: t('import.archive'),
            hint: t('import.archiveHint'),
            keywords: t('settings.keywords.import'),
            render: () => <ArchiveImport />,
          },
        ],
      },
      {
        id: 'connector',
        title: t('connector.section'),
        fields: [
          {
            id: 'access',
            label: t('access.label'),
            keywords: t('settings.keywords.connector'),
            render: () => <LibraryAccess access={server?.access} />,
          },
          {
            id: 'connector',
            label: t('connector.label'),
            hint: t('connector.hint'),
            keywords: t('settings.keywords.connector'),
            render: () => <BrowserConnector status={server?.connector} onChange={refreshServer} />,
          },
        ],
      },
      {
        id: 'addin',
        title: t('addin.section'),
        fields: [
          {
            id: 'addin',
            label: t('addin.label'),
            hint: t('addin.hint'),
            keywords: t('settings.keywords.addin'),
            render: () => <WordAddin />,
          },
        ],
      },
      {
        id: 'maintenance',
        title: t('settings.maintenance'),
        fields: [
          {
            id: 'maintenance',
            label: t('settings.maintenance'),
            hint: t('settings.maintenanceNote'),
            keywords: t('settings.keywords.maintenance'),
            render: () => (
              <div className="button-row">
                <Button
                  onClick={() =>
runReindex()
                  }
                >
                  {t('menu.reindex')}
                </Button>
                <Button
                  onClick={() =>
runOptimize()
                  }
                >
                  {t('statusPage.optimize')}
                </Button>
                <Button onClick={() => void runBackup()}>{t('settings.backup')}</Button>
                <Button onClick={() => void runIntegrity()}>{t('settings.integrity')}</Button>
                <Button onClick={() => void runExportAll()}>{t('settings.exportAll')}</Button>
              </div>
            ),
          },
        ],
      },
      {
        id: 'about',
        title: t('settings.about'),
        fields: [
          {
            id: 'about',
            label: t('settings.about'),
            keywords: t('settings.keywords.about'),
            render: () => (
              <dl className="kv">
                <dt>Yinkote</dt>
                <dd>{server?.version ?? '—'}</dd>
                <dt>{t('statusPage.provider')}</dt>
                <dd>
                  {stats?.search.provider ?? '—'}
                  {/* What it means for results, because the name says nothing
                      to anybody who has not read the source. */}
                  <div className="dim">{embedderMeaning(t, stats?.search.provider)}</div>
                </dd>
                <dt>{t('settings.license')}</dt>
                {/* i18n-exempt: an SPDX identifier is machine-readable metadata, not prose */}
                <dd>AGPL-3.0-or-later</dd>
              </dl>
            ),
          },
        ],
      },
    ],
    [
      t,
      locale,
      theme,
      accent,
      density,
      citationStyle,
      citationStyles,
      mode,
      sources,
      server,
      stats,
      agent,
      configureAgent,
      setLocale,
      setTheme,
      setDensity,
      setCitationStyle,
    ],
  )

  const visible = useMemo(() => filterSettings(sections, filter), [sections, filter])

  const jump = (id: string) => {
    const target = bodyRef.current?.querySelector(`[data-section="${id}"]`)
    target?.scrollIntoView({ block: 'start', behavior: 'smooth' })
    setCurrent(id)
  }

  /** Highlight whichever section the reader is actually looking at.
   *
   *  An observer rather than a scroll handler: it reports only when a section
   *  crosses the band, so the rail is not recomputed on every scroll frame. */
  useEffect(() => {
    const root = bodyRef.current
    if (!root) return
    const sections = [...root.querySelectorAll('[data-section]')]
    if (!sections.length) return

    const observer = new IntersectionObserver(
      (entries) => {
        const hit = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0]
        if (hit) setCurrent(hit.target.getAttribute('data-section'))
      },
      // A band across the top: whatever has just reached the reading position.
      { root, rootMargin: '0px 0px -70% 0px', threshold: 0 },
    )
    sections.forEach((s) => observer.observe(s))
    return () => observer.disconnect()
  }, [visible])

  return (
    <div className="settings">
      <nav className="settings-rail">
        <div className="search settings-search">
          <Icon.Search size={12} className="search-icon" />
          <Input
            value={filter}
            autoFocus
            placeholder={t('settings.filter')}
            onChange={(e) => setFilter(e.target.value)}
          />
        </div>
        {visible.map((section) => (
          <button
            key={section.id}
            className="nav-item"
            data-active={current === section.id}
            onClick={() => jump(section.id)}
          >
            <span className="label">{section.title}</span>
            <span className="count">{section.fields.length}</span>
          </button>
        ))}
      </nav>

      <div className="settings-body page narrow" ref={bodyRef}>
        {visible.length === 0 && <div className="empty">{t('settings.noMatches')}</div>}
        {visible.map((section) => (
          <div key={section.id} data-section={section.id}>
            <Section title={section.title}>
              {section.fields.map((field) => (
                <Field key={field.id} label={field.label} hint={field.hint}>
                  {field.render()}
                </Field>
              ))}
            </Section>
          </div>
        ))}
      </div>
    </div>
  )
}

/** Pointing the assistant at a model.
 *
 *  The program is a local server the user started, so this has to be doable
 *  from the workbench; telling somebody to edit a TOML file and restart makes
 *  the web interface a partial one.
 *
 *  The key is write-only. It is never sent back, so the box stays empty and
 *  says whether one is stored — leaving it alone keeps it, and clearing it
 *  explicitly removes it.
 */
function ModelSettings({
  agent,
  onSave,
}: {
  agent: AgentStatus | null
  onSave: (patch: {
    endpoint?: string
    model?: string
    apiKey?: string
    allowCommands?: boolean
  }) => Promise<void>
}) {
  const t = useT()
  const [endpoint, setEndpoint] = useState('')
  const [model, setModel] = useState('')
  const [apiKey, setApiKey] = useState('')
  const [saving, setSaving] = useState(false)

  // Seeded from the server rather than mirrored: a field the user is typing
  // in must not be overwritten by a status refresh.
  useEffect(() => {
    setEndpoint(agent?.endpoint ?? '')
    setModel(agent?.model ?? '')
  }, [agent?.endpoint, agent?.model])

  const save = async () => {
    setSaving(true)
    try {
      await onSave({
        endpoint,
        model,
        // Absent means "leave it"; the box is empty because the key is never
        // shown, not because the user cleared it.
        ...(apiKey ? { apiKey } : {}),
      })
      setApiKey('')
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="model-settings">
      <Input
        value={endpoint}
        placeholder="http://127.0.0.1:11434/v1"
        onChange={(e) => setEndpoint(e.target.value)}
      />
      <Input
        value={model}
        placeholder={t('settings.modelName')}
        onChange={(e) => setModel(e.target.value)}
      />
      <Input
        value={apiKey}
        type="password"
        placeholder={agent?.hasApiKey ? t('settings.keyStored') : t('settings.keyOptional')}
        onChange={(e) => setApiKey(e.target.value)}
      />
      <div className="row-actions">
        <Button
          tone="primary"
          disabled={saving || !endpoint.trim() || !model.trim()}
          onClick={() => void save()}
        >
          {t('settings.modelSave')}
        </Button>
        <Badge tone={agent?.configured ? 'ok' : 'muted'}>
          {agent?.configured ? t('settings.modelReady') : t('settings.modelUnset')}
        </Badge>
      </div>
      {agent?.configured && (
        <p className="dim">{t('settings.modelTools', { count: agent.tools?.length ?? 0 })}</p>
      )}
    </div>
  )
}

/** Which skills the assistant is offered.
 *
 *  A deny list underneath: a skill dropped into the folder works without being
 *  switched on anywhere, and turning one off is the exception worth recording.
 */
function SkillToggles({
  agent,
  onSave,
}: {
  agent: AgentStatus | null
  onSave: (patch: { disabledSkills?: string[] }) => Promise<void>
}) {
  const t = useT()
  const skills = agent?.skills ?? []

  if (!skills.length) return <span className="ctl-static dim">{t('settings.skillsNone')}</span>

  const toggle = (name: string, on: boolean) => {
    const off = skills.filter((s) => (s.name === name ? !on : !s.enabled)).map((s) => s.name)
    void onSave({ disabledSkills: off })
  }

  return (
    <div className="toggle-list">
      {skills.map((skill) => (
        <label key={skill.name} className="toggle-row" title={skill.description}>
          <input
            type="checkbox"
            checked={skill.enabled}
            onChange={(e) => toggle(skill.name, e.target.checked)}
          />
          <span className="toggle-name">{skill.name}</span>
          <span className="toggle-hint dim">{skill.description}</span>
        </label>
      ))}
    </div>
  )
}

/** Which tools the assistant is given.
 *
 *  The catalogue comes from the server, so a tool added later appears here
 *  without anybody maintaining a second list. Writing tools are marked,
 *  because "may it change my library?" is the question actually being asked.
 */
function ToolToggles({
  agent,
  onSave,
}: {
  agent: AgentStatus | null
  onSave: (patch: { disabledTools?: string[] }) => Promise<void>
}) {
  const t = useT()
  const all = agent?.allTools ?? []
  const disabled = agent?.disabledTools ?? []
  const writes = agent?.writes ?? []

  if (!all.length) return <span className="ctl-static dim">{t('settings.toolsNone')}</span>

  const toggle = (name: string, on: boolean) => {
    const off = on ? disabled.filter((d) => d !== name) : [...disabled, name]
    void onSave({ disabledTools: off })
  }

  return (
    <div className="toggle-list tools">
      {all.map((name) => (
        <label key={name} className="toggle-row">
          <input
            type="checkbox"
            checked={!disabled.includes(name)}
            onChange={(e) => toggle(name, e.target.checked)}
          />
          <span className="toggle-name mono">{name}</span>
          {writes.includes(name) && <Badge tone="warn">{t('settings.toolWrites')}</Badge>}
        </label>
      ))}
    </div>
  )
}
