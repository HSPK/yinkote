import { Component, type ErrorInfo, type ReactNode } from 'react'

import { t } from '../i18n'
import { Button } from './controls'

interface Props {
  /** Changing this resets the boundary — used to retry on a different tab. */
  resetKey?: string
  children: ReactNode
}

interface State {
  error: Error | null
}

/**
 * Keeps one broken surface from taking the window with it.
 *
 * React unmounts the whole tree when a render throws, so without this a single
 * bad value blanks the workbench — no sidebar, no tabs, no way back, and for a
 * tool that stays open all day that is a much worse outcome than the bug that
 * caused it. The blast radius belongs around each surface: a reader that cannot
 * draw a page should not cost you the library beside it.
 *
 * Deliberately a class, since this is the one thing hooks cannot express.
 */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null }

  static getDerivedStateFromError(error: Error): State {
    return { error }
  }

  componentDidUpdate(previous: Props) {
    // Moving to another tab is a fresh attempt; staying put is not, or the
    // boundary would loop on the same failure.
    if (previous.resetKey !== this.props.resetKey && this.state.error) {
      this.setState({ error: null })
    }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('surface failed', error, info.componentStack)
  }

  render() {
    const { error } = this.state
    if (!error) return this.props.children

    return (
      <div className="pane main surface-error">
        <div className="surface-error-body">
          <strong>{t('error.surface')}</strong>
          <p className="note">{error.message}</p>
          <Button onClick={() => this.setState({ error: null })}>{t('error.retry')}</Button>
        </div>
      </div>
    )
  }
}
