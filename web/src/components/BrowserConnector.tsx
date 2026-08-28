import { useState } from 'react'

import { api } from '../api/client'
import type { AccessState, ConnectorStatus } from '../api/types'
import { useT } from '../i18n'
import { Badge, Button, toast } from '../ui'

/** The port the Zotero extensions look for. Not ours to choose. */
const ZOTERO_PORT = 23119

/** Reachable with no key is the one worth saying out loud, every time. */
const ACCESS_TONE = { private: 'ok', protected: 'ok', open: 'warn' } as const

/**
 * Who can reach this library.
 *
 * A server past loopback with no key is a deliberate choice — it refuses to
 * start otherwise — but the choice was made once, very likely in a service
 * file, and then never shown again. The state that carries the risk looked
 * exactly like the state that does not.
 */
export function LibraryAccess({ access }: { access?: AccessState }) {
  const t = useT()
  if (!access) return <span className="muted">{t('settings.loading')}</span>
  return (
    <div>
      <div className="chip-row tight">
        <Badge tone={ACCESS_TONE[access.state]}>{t(`access.state.${access.state}`)}</Badge>
      </div>
      <p className="muted">{t(`access.hint.${access.state}`)}</p>
    </div>
  )
}

/** Listening is good news; asked-for-but-refused is the one worth a warning. */
const TONE = { listening: 'ok', unavailable: 'warn', off: undefined } as const

/**
 * Saving to this library from the browser.
 *
 * The server already speaks Zotero's connector protocol, which is what the
 * Zotero browser extensions talk to — so they work unchanged and there is
 * nothing of ours to install. It listens only when asked, though, because that
 * port belongs to Zotero and taking it would break a Zotero the user is still
 * migrating away from.
 *
 * The result was a headline feature that existed, was off, and said so
 * nowhere. This is the missing sentence.
 *
 * It reports what the port is *doing*, not what was requested: the bind is
 * allowed to fail and the server carries on, so "asked for" and "working" are
 * different facts and only one of them is worth telling somebody.
 */
export function BrowserConnector({
  status,
  onChange,
}: {
  status?: ConnectorStatus
  onChange?: () => void
}) {
  const t = useT()
  const [busy, setBusy] = useState(false)

  if (!status) return <span className="muted">{t('settings.loading')}</span>

  const on = status.state === 'listening'

  // Answers with the resulting status rather than an acknowledgement, because
  // binding can fail: a running Zotero owns 23119, and that has to be said now
  // rather than discovered the next time the user saves a page.
  const toggle = async () => {
    setBusy(true)
    try {
      const next = await api.connector.set(on ? null : ZOTERO_PORT)
      toast.success(t(`connector.state.${next.state}`), t(`connector.hint.${next.state}`))
      onChange?.()
    } catch (error) {
      toast.fromError(t('connector.failed'), error)
    } finally {
      setBusy(false)
    }
  }

  return (
    <div>
      <div className="chip-row tight">
        <Badge tone={TONE[status.state]}>{t(`connector.state.${status.state}`)}</Badge>
        {status.state !== 'off' && <code>127.0.0.1:{status.port}</code>}
        <Button onClick={() => void toggle()} disabled={busy}>
          {t(on ? 'connector.turnOff' : 'connector.turnOn')}
        </Button>
      </div>
      <p className="muted">{t(`connector.hint.${status.state}`)}</p>
    </div>
  )
}
