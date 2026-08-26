/** Translation.
 *
 *  Deliberately tiny: a flat key/string map per locale, `{placeholder}`
 *  interpolation, and a store so a language switch re-renders the app. The
 *  reference locale defines the key type, so a locale that drifts fails the
 *  build rather than showing `[missing]` to a user.
 */
import { create } from 'zustand'

import { enUS } from './en-US'
import { zhCN, type MessageKey } from './zh-CN'

export type Locale = 'zh-CN' | 'en-US'

export const LOCALES: { value: Locale; label: string }[] = [
  { value: 'zh-CN', label: '简体中文' },
  { value: 'en-US', label: 'English' },
]

const MESSAGES: Record<Locale, Record<MessageKey, string>> = {
  'zh-CN': zhCN,
  'en-US': enUS,
}

export type Vars = Record<string, string | number>

export function format(template: string, vars?: Vars): string {
  if (!vars) return template
  return template.replace(/\{(\w+)\}/g, (whole, name: string) =>
    name in vars ? String(vars[name]) : whole,
  )
}

/** Pick the closest supported locale for a browser language tag. */
export function detectLocale(languages: readonly string[] = navigator.languages ?? []): Locale {
  for (const tag of languages) {
    const lower = tag.toLowerCase()
    if (lower.startsWith('zh')) return 'zh-CN'
    if (lower.startsWith('en')) return 'en-US'
  }
  return 'zh-CN'
}

interface I18nState {
  locale: Locale
  setLocale: (locale: Locale) => void
}

export const useI18n = create<I18nState>((set) => ({
  locale: detectLocale(),
  setLocale(locale) {
    set({ locale })
    document.documentElement.lang = locale
  },
}))

/** Translate outside React (menus, toasts, store actions). */
export function t(key: MessageKey, vars?: Vars): string {
  return format(MESSAGES[useI18n.getState().locale][key], vars)
}

/** Translate inside React, re-rendering when the language changes. */
export function useT(): (key: MessageKey, vars?: Vars) => string {
  const locale = useI18n((s) => s.locale)
  return (key, vars) => format(MESSAGES[locale][key], vars)
}

/** Anything the server labels in both languages. */
export interface Labelled {
  label: string
  labelEn: string
}

/** Pick the right side of a server-supplied label pair.
 *
 *  Item types and field names come from the schema, not from these catalogues,
 *  so they need translating too — otherwise English mode shows a fully English
 *  chrome wrapped around Chinese type names.
 */
export function schemaLabel(def: Labelled | undefined, locale: Locale, fallback = ''): string {
  if (!def) return fallback
  return (locale === 'en-US' ? def.labelEn : def.label) || def.label || fallback
}

/** `schemaLabel` bound to the current locale, for use inside React. */
export function useSchemaLabel(): (def: Labelled | undefined, fallback?: string) => string {
  const locale = useI18n((s) => s.locale)
  return (def, fallback = '') => schemaLabel(def, locale, fallback)
}

export type { MessageKey }
