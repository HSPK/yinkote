/** Icon set.
 *
 *  Hand-drawn on a 16×16 grid with a single stroke weight so they sit together
 *  as a family — the unicode glyphs they replace were borrowed from five
 *  different typefaces and looked it. `currentColor` throughout, so an icon is
 *  coloured by whatever it sits inside.
 */
import type { SVGProps } from 'react'

type IconProps = SVGProps<SVGSVGElement> & { size?: number }

/** Inline SVGs sit on the text baseline, which leaves a descender's worth of
 *  space beneath them and pushes them off centre inside a fixed-size button. */
const BLOCK = { display: 'block', flex: 'none' } as const

function Svg({ size = 14, children, ...props }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.3}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      style={BLOCK}
      {...props}
    >
      {children}
    </svg>
  )
}

export const Icon = {
  /** Three vertical bands: the column picker. */
  Columns: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2 3h3.2v10H2zM6.4 3h3.2v10H6.4zM10.8 3H14v10h-3.2z" />
    </Svg>
  ),
  /** A frame with a docked side panel. */
  Panel: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2 3h12v10H2zm1.4 1.4v7.2h5.4V4.4z" />
    </Svg>
  ),
  /** Stack of shelved volumes. */
  Library: (p: IconProps) => (
    <Svg {...p}>
      <path d="M3 2.5h2.2v11H3zM6.6 2.5h2.2v11H6.6z" />
      <path d="m10.4 3.2 2.1-.5 2 10.6-2.1.5z" />
    </Svg>
  ),
  Trash: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2.8 4.2h10.4M6.4 4.2V2.8h3.2v1.4M4.2 4.2l.7 9h6.2l.7-9" />
      <path d="M6.6 6.6v4.2M9.4 6.6v4.2" />
    </Svg>
  ),
  Folder: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2 4.2a1 1 0 0 1 1-1h3l1.4 1.6H13a1 1 0 0 1 1 1v6.4a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1z" />
    </Svg>
  ),
  FolderOpen: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2 4.2a1 1 0 0 1 1-1h3l1.4 1.6H13a1 1 0 0 1 1 1v1.4H2z" />
      <path d="M2 7.2h12.4l-1.3 5a1 1 0 0 1-1 .8H3a1 1 0 0 1-1-1z" />
    </Svg>
  ),
  /** Funnel: a saved query filters the library. */
  Smart: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2.4 3.2h11.2l-4.3 5v4.4l-2.6 1.2V8.2z" />
    </Svg>
  ),
  Tag: (p: IconProps) => (
    <Svg {...p}>
      <path d="M8.4 2.2H13a.8.8 0 0 1 .8.8v4.6a1 1 0 0 1-.3.7l-5.6 5.6a1 1 0 0 1-1.4 0L2.3 9.5a1 1 0 0 1 0-1.4l5.4-5.6a1 1 0 0 1 .7-.3z" />
      <circle cx="11" cy="5" r="1" fill="currentColor" stroke="none" />
    </Svg>
  ),
  Chat: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2.2 3.6a1 1 0 0 1 1-1h9.6a1 1 0 0 1 1 1v6.2a1 1 0 0 1-1 1H6.6L3.4 13.4v-2.6h-.2a1 1 0 0 1-1-1z" />
    </Svg>
  ),
  Search: (p: IconProps) => (
    <Svg {...p}>
      <circle cx="7" cy="7" r="4.4" />
      <path d="m10.3 10.3 3.2 3.2" />
    </Svg>
  ),
  Plugin: (p: IconProps) => (
    <Svg {...p}>
      <path d="M6 2.4v2.2H3.4a1 1 0 0 0-1 1v2.2h1.4a1.6 1.6 0 1 1 0 3.2H2.4v2.2a1 1 0 0 0 1 1h2.2v-1.4a1.6 1.6 0 1 1 3.2 0v1.4h2.2a1 1 0 0 0 1-1v-2.6h1.4a1.6 1.6 0 1 0 0-3.2h-1.4V5.6a1 1 0 0 0-1-1H9.2V2.4" />
    </Svg>
  ),
  Gauge: (p: IconProps) => (
    <Svg {...p}>
      <path d="M2.2 11.4a6 6 0 1 1 11.6 0" />
      <path d="M8 11.4 11 6.6" />
      <circle cx="8" cy="11.6" r="1.1" fill="currentColor" stroke="none" />
    </Svg>
  ),
  /** A cogwheel: eight teeth on a 16-grid, which is as many as stay legible. */
  Settings: (p: IconProps) => (
    <Svg {...p}>
      <circle cx="8" cy="8" r="2.1" />
      <path d="M8 1.3a6.7 6.7 0 0 1 1.9.28l.26 1.6a4.9 4.9 0 0 1 1.1.64l1.5-.6a6.7 6.7 0 0 1 1.35 2.34l-1.24 1.05a4.9 4.9 0 0 1 0 1.28l1.24 1.05a6.7 6.7 0 0 1-1.35 2.34l-1.5-.6a4.9 4.9 0 0 1-1.1.64l-.26 1.6a6.7 6.7 0 0 1-3.8 0l-.26-1.6a4.9 4.9 0 0 1-1.1-.64l-1.5.6A6.7 6.7 0 0 1 1.89 9.64l1.24-1.05a4.9 4.9 0 0 1 0-1.28L1.89 6.26A6.7 6.7 0 0 1 3.24 3.92l1.5.6a4.9 4.9 0 0 1 1.1-.64l.26-1.6A6.7 6.7 0 0 1 8 1.3Z" />
    </Svg>
  ),
  Star: (p: IconProps) => (
    <Svg {...p}>
      <path d="m8 2 1.85 3.75 4.15.6-3 2.93.71 4.12L8 11.95 4.29 13.4 5 9.28 2 6.35l4.15-.6z" />
    </Svg>
  ),
  Flask: (p: IconProps) => (
    <Svg {...p}>
      <path d="M6.4 1.8v4L2.9 12a1.4 1.4 0 0 0 1.2 2.1h7.8a1.4 1.4 0 0 0 1.2-2.1L9.6 5.8v-4M5.4 1.8h5.2M4.6 9.6h6.8" />
    </Svg>
  ),
  Plus: (p: IconProps) => (
    <Svg {...p}>
      <path d="M8 3.4v9.2M3.4 8h9.2" />
    </Svg>
  ),
  Close: (p: IconProps) => (
    <Svg {...p}>
      <path d="m4 4 8 8M12 4l-8 8" />
    </Svg>
  ),
  ChevronRight: (p: IconProps) => (
    <Svg {...p}>
      <path d="m6 3.6 4.4 4.4L6 12.4" />
    </Svg>
  ),
  ChevronDown: (p: IconProps) => (
    <Svg {...p}>
      <path d="M3.6 6 8 10.4 12.4 6" />
    </Svg>
  ),
  Dot: (p: IconProps) => (
    <Svg {...p}>
      <circle cx="8" cy="8" r="2" fill="currentColor" stroke="none" />
    </Svg>
  ),
}

export type IconName = keyof typeof Icon
