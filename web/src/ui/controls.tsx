/** Shared form and layout controls.
 *
 *  Deliberately thin wrappers over native elements: they exist to make the
 *  design language impossible to get wrong, not to reinvent the DOM.
 */
import type { ReactNode, SelectHTMLAttributes } from 'react'

type ButtonTone = 'default' | 'primary' | 'danger' | 'ghost'

export function Button({
  tone = 'default',
  children,
  ...props
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { tone?: ButtonTone }) {
  return (
    <button {...props} className={`btn btn-${tone} ${props.className ?? ''}`.trim()}>
      {children}
    </button>
  )
}

export function Input(props: React.InputHTMLAttributes<HTMLInputElement>) {
  return <input spellCheck={false} {...props} className={`ctl ${props.className ?? ''}`.trim()} />
}

export function Textarea(props: React.TextareaHTMLAttributes<HTMLTextAreaElement>) {
  return <textarea spellCheck={false} {...props} className={`ctl ${props.className ?? ''}`.trim()} />
}

export function Select({
  options,
  ...props
}: SelectHTMLAttributes<HTMLSelectElement> & { options: { value: string; label: string }[] }) {
  return (
    <select {...props} className={`ctl ${props.className ?? ''}`.trim()}>
      {options.map((o) => (
        <option key={o.value} value={o.value}>
          {o.label}
        </option>
      ))}
    </select>
  )
}

/** Label + control + hint, so every form in the app lines up identically. */
export function Field({
  label,
  hint,
  children,
}: {
  label: string
  hint?: ReactNode
  children: ReactNode
}) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      {children}
      {hint && <span className="field-hint">{hint}</span>}
    </label>
  )
}

export function Section({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  return (
    <section className="section">
      <header className="section-head">
        <h2>{title}</h2>
        {action}
      </header>
      <div className="section-body">{children}</div>
    </section>
  )
}

export function Empty({ children, title }: { children: ReactNode; title?: string }) {
  return (
    <div className="empty" title={title}>
      {children}
    </div>
  )
}

export function Badge({
  tone,
  children,
  ...props
}: React.HTMLAttributes<HTMLSpanElement> & { tone?: string; children: ReactNode }) {
  return (
    <span {...props} className={`badge ${props.className ?? ''}`.trim()} data-tone={tone}>
      {children}
    </span>
  )
}

export interface ToggleProps {
  checked: boolean
  disabled?: boolean
  onChange: (checked: boolean) => void
}

/** A switch. Reads as on/off at a glance, which a checkbox at this size does not. */
export function Toggle({ checked, disabled, onChange }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      className="toggle"
      onClick={() => onChange(!checked)}
    >
      <span className="toggle-knob" />
    </button>
  )
}
