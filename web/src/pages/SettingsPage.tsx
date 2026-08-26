import { useEffect, useState } from 'react'

import { api } from '../api/client'
import type { SearchMode, SourceInfo } from '../api/types'
import { LOCALES, useI18n, useT, type Locale } from '../i18n'
import { THEMES, isHexColour } from '../lib/theme'
import { useStore } from '../state/store'
import { Badge, Button, Field, Section, Select, toast, withToast } from '../ui'

const MODES: SearchMode[] = ['hybrid', 'keyword', 'fuzzy', 'semantic']
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
  const setMode = useStore((s) => s.setMode)
  const setDensity = useStore((s) => s.setDensity)
  const setTheme = useStore((s) => s.setTheme)
  const setLocale = useStore((s) => s.setLocale)

  const [sources, setSources] = useState<SourceInfo[]>([])

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

  return (
    <div className="page narrow">
      <Section title={t('settings.appearance')}>
        <Field label={t('settings.language')}>
          <Select
            value={locale}
            options={LOCALES.map((l) => ({ value: l.value, label: l.label }))}
            onChange={(e) => setLocale(e.target.value as Locale)}
          />
        </Field>

        <Field label={t('settings.theme')}>
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
        </Field>

        <Field label="Accent">
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
                {t('dialog.cancel')}
              </Button>
            )}
          </div>
        </Field>

        <Field label={t('settings.density')}>
          <Select
            value={density}
            options={DENSITIES.map((d) => ({ value: d, label: t(`settings.density.${d}`) }))}
            onChange={(e) => setDensity(e.target.value)}
          />
        </Field>
      </Section>

      <Section title={t('settings.search')}>
        <Field label={t('settings.defaultMode')} hint={t('settings.defaultModeHint')}>
          <Select
            value={mode}
            options={MODES.map((m) => ({ value: m, label: t(`search.mode.${m}`) }))}
            onChange={(e) => setMode(e.target.value as SearchMode)}
          />
        </Field>
        <Field label={t('settings.syntax')} hint={t('settings.syntaxHint')}>
          <div className="syntax">
            <code>tag:survey</code>
            <code>-tag:obsolete</code>
            <code>type:book</code>
            <code>author:zhang</code>
            <code>year:2020..2024</code>
            <code>&quot;exact phrase&quot;</code>
          </div>
        </Field>
      </Section>

      <Section title={t('settings.quickAdd')}>
        <Field label={t('settings.resolvers')} hint={t('settings.resolversHint')}>
          <div className="chip-row tight">
            {sources.map((s) => (
              <Badge key={s.id} tone="accent" title={s.supports.join(' / ')}>
                {s.label}
              </Badge>
            ))}
            {sources.length === 0 && <span className="muted">{t('settings.loading')}</span>}
          </div>
        </Field>
      </Section>

      <Section title={t('settings.storage')}>
        <Field label={t('settings.dataDir')} hint={t('settings.dataDirHint')}>
          <div className="path-row">
            <code>{server?.dataDir ?? '—'}</code>
            <Button tone="ghost" disabled={!server} onClick={() => void copy(server?.dataDir ?? '')}>
              {t('settings.copy')}
            </Button>
          </div>
        </Field>
        <Field label={t('settings.pluginDirs')}>
          <ul className="path-list">
            {(server?.pluginDirs ?? []).map((d) => (
              <li key={d}>{d}</li>
            ))}
          </ul>
        </Field>
      </Section>

      <Section title={t('settings.maintenance')}>
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
        <p className="note">{t('settings.maintenanceNote')}</p>
      </Section>

      <Section title={t('settings.about')}>
        <dl className="kv">
          <dt>Yinkote</dt>
          <dd>{server?.version ?? '—'}</dd>
          <dt>{t('statusPage.provider')}</dt>
          <dd>{stats?.search.provider ?? '—'}</dd>
          <dt>{t('settings.license')}</dt>
          <dd>AGPL-3.0-or-later</dd>
        </dl>
      </Section>
    </div>
  )
}
