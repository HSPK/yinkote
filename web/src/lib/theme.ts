/** Themes.
 *
 *  A theme is just a set of CSS custom properties written onto `:root`. That
 *  keeps every stylesheet theme-agnostic — no per-theme selectors, no cascade
 *  fights — and lets a user-defined accent colour be layered on top of any
 *  preset with one more property write.
 */

export interface Theme {
  id: string
  /** Untranslated on purpose: theme names are proper nouns. */
  name: string
  dark: boolean
  vars: Record<string, string>
}

/** Only the properties a theme is allowed to change; everything else is
 *  structural and stays fixed so layout cannot drift between themes. */
const KEYS = [
  '--bg',
  '--bg-1',
  '--bg-2',
  '--bg-3',
  '--line',
  '--line-2',
  '--fg',
  '--fg-dim',
  '--fg-mute',
  '--accent',
  '--accent-dim',
  '--ok',
  '--warn',
  '--err',
  '--mark',
] as const

export const THEMES: Theme[] = [
  {
    id: 'industrial',
    name: 'Industrial',
    dark: true,
    vars: {
      '--bg': '#0d1013',
      '--bg-1': '#14181c',
      '--bg-2': '#1a1f24',
      '--bg-3': '#222930',
      '--line': '#2a323a',
      '--line-2': '#384250',
      '--fg': '#d5dde5',
      '--fg-dim': '#8b98a5',
      '--fg-mute': '#5d6b78',
      '--accent': '#4da3ff',
      '--accent-dim': '#1f5c99',
      '--ok': '#4ec9a0',
      '--warn': '#e2b341',
      '--err': '#f2635f',
      '--mark': '#6b5a1f',
    },
  },
  {
    id: 'graphite',
    name: 'Graphite',
    dark: true,
    vars: {
      '--bg': '#101010',
      '--bg-1': '#171717',
      '--bg-2': '#1e1e1e',
      '--bg-3': '#282828',
      '--line': '#303030',
      '--line-2': '#404040',
      '--fg': '#dcdcdc',
      '--fg-dim': '#9a9a9a',
      '--fg-mute': '#6a6a6a',
      '--accent': '#c9a227',
      '--accent-dim': '#6b5512',
      '--ok': '#7ea86b',
      '--warn': '#d09a3c',
      '--err': '#c9564f',
      '--mark': '#5c4a12',
    },
  },
  {
    id: 'blueprint',
    name: 'Blueprint',
    dark: true,
    vars: {
      '--bg': '#0a1220',
      '--bg-1': '#0f1a2c',
      '--bg-2': '#142238',
      '--bg-3': '#1c2d46',
      '--line': '#22374f',
      '--line-2': '#2f4a68',
      '--fg': '#cfe0f5',
      '--fg-dim': '#8aa3c0',
      '--fg-mute': '#5b7590',
      '--accent': '#5ec8f2',
      '--accent-dim': '#1d5f7d',
      '--ok': '#54c7a8',
      '--warn': '#e0b24b',
      '--err': '#ef6b6b',
      '--mark': '#4c5f1d',
    },
  },
  {
    id: 'paper',
    name: 'Paper',
    dark: false,
    vars: {
      '--bg': '#f4f4f1',
      '--bg-1': '#fbfbf9',
      '--bg-2': '#eeeeea',
      '--bg-3': '#e2e2dd',
      '--line': '#d3d3cc',
      '--line-2': '#b9b9b0',
      '--fg': '#1f2328',
      '--fg-dim': '#4d5560',
      '--fg-mute': '#7c8794',
      '--accent': '#1a5fb4',
      '--accent-dim': '#3b7dd8',
      '--ok': '#1f7a5c',
      '--warn': '#8a6100',
      '--err': '#b3261e',
      '--mark': '#ffe9a8',
    },
  },
  {
    id: 'contrast',
    name: 'High Contrast',
    dark: true,
    vars: {
      '--bg': '#000000',
      '--bg-1': '#000000',
      '--bg-2': '#0d0d0d',
      '--bg-3': '#1a1a1a',
      '--line': '#4d4d4d',
      '--line-2': '#7a7a7a',
      '--fg': '#ffffff',
      '--fg-dim': '#d6d6d6',
      '--fg-mute': '#a0a0a0',
      '--accent': '#4dc3ff',
      '--accent-dim': '#005f87',
      '--ok': '#3fe08c',
      '--warn': '#ffd23f',
      '--err': '#ff6b6b',
      '--mark': '#7a6400',
    },
  },
]

export const DEFAULT_THEME = 'industrial'

export function findTheme(id: string): Theme {
  return THEMES.find((t) => t.id === id) ?? THEMES[0]!
}

/** Slightly darken a hex colour, used to derive the accent's paired shade. */
function shade(hex: string, factor: number): string {
  const value = hex.replace('#', '')
  if (value.length !== 6) return hex
  const channels = [0, 2, 4].map((i) => {
    const n = Number.parseInt(value.slice(i, i + 2), 16)
    return Math.max(0, Math.min(255, Math.round(n * factor)))
  })
  return `#${channels.map((c) => c.toString(16).padStart(2, '0')).join('')}`
}

export function isHexColour(value: string): boolean {
  return /^#[0-9a-fA-F]{6}$/.test(value)
}

/**
 * Write a theme onto the document.
 *
 * `accent` overrides the preset's accent so a user can keep a familiar palette
 * and still make the app theirs; its companion shade is derived rather than
 * asked for, because nobody wants to pick two colours.
 */
export function applyTheme(id: string, accent?: string): void {
  const theme = findTheme(id)
  const root = document.documentElement
  for (const key of KEYS) {
    const value = theme.vars[key]
    if (value) root.style.setProperty(key, value)
  }
  if (accent && isHexColour(accent)) {
    root.style.setProperty('--accent', accent)
    root.style.setProperty('--accent-dim', shade(accent, theme.dark ? 0.45 : 1.15))
  }
  root.dataset.theme = theme.id
  root.style.setProperty('color-scheme', theme.dark ? 'dark' : 'light')
}
