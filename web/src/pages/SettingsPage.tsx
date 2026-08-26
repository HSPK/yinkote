import { useEffect, useMemo, useRef, useState } from 'react'

import { api } from '../api/client'
import type { SourceInfo } from '../api/types'
import { LOCALES, useI18n, useT, type Locale } from '../i18n'
import { filterSettings, type SettingSection } from '../lib/settings'
import { THEMES, isHexColour } from '../lib/theme'
import { useStore } from '../state/store'
import { Badge, Button, Field, Icon, Input, Section, Select, toast, withToast } from '../ui'

const DENSITIES = ['compact', 'comfortable'] as const

export function SettingsPage() {
  const t = useT()
  const locale = useI18n((s) => s.locale)

  const server = useStore((s) => s.server)
  const stats = useStore((s) => s.stats)
  const mode = useStore((s) => s.mode)
  const density = useStore((s) => s.density)
  const theme = useStore((s) => s.theme)
  const accent = useStore((s) => s.accent)
  const setDensity = useStore((s) => s.setDensity)
  const setTheme = useStore((s) => s.setTheme)
  const setLocale = useStore((s) => s.setLocale)

  const [sources, setSources] = useState<SourceInfo[]>([])
  const [filter, setFilter] = useState('')
  const bodyRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    api.scrape
      .sources()
      .then(setSources)
      .catch(() => setSources([]))
  }, [])

  const copy = async (value: string) => {
    await navigator.clipboard.writeText(value)
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
              <div className="accent-row">
                <input
                  type="color"
                  className="accent-picker"
                  value={isHexColour(accent) ? accent : '#4da3ff'}
                  onChange={(e) => setTheme(theme, e.target.value)}
                />
                <code>{accent || '—'}</code>
                {accent && (
                  <Button tone="ghost" onClick={() => setTheme(theme, '')}>
                    {t('settings.reset')}
                  </Button>
                )}
              </div>
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
                    withToast(useStore.getState().reindex, {
                      success: t('toast.reindexed'),
                      failure: t('toast.reindexFailed'),
                    })
                  }
                >
                  {t('menu.reindex')}
                </Button>
                <Button
                  onClick={() =>
                    withToast(useStore.getState().optimize, {
                      success: t('toast.optimized'),
                      failure: t('toast.optimizeFailed'),
                    })
                  }
                >
                  {t('statusPage.optimize')}
                </Button>
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
                <dd>{stats?.search.provider ?? '—'}</dd>
                <dt>{t('settings.license')}</dt>
                <dd>AGPL-3.0-or-later</dd>
              </dl>
            ),
          },
        ],
      },
    ],
    [t, locale, theme, accent, density, mode, sources, server, stats, setLocale, setTheme, setDensity],
  )

  const visible = useMemo(() => filterSettings(sections, filter), [sections, filter])

  const jump = (id: string) => {
    const target = bodyRef.current?.querySelector(`[data-section="${id}"]`)
    target?.scrollIntoView({ block: 'start', behavior: 'smooth' })
  }

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
          <button key={section.id} className="nav-item" onClick={() => jump(section.id)}>
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
