import { useState } from 'react'

import { useT } from '../i18n'
import { useStore } from '../state/store'
import { Button } from '../ui'

/**
 * Asking for the key a server was started with.
 *
 * A server run with `YK_API_KEY` answers 401 to everything, and the workbench
 * sent no key at all — so the page loaded and every request failed, for ever,
 * behind the word "connecting". The only documented way to expose a library
 * safely made the product unusable, which left `--allow-anonymous` as the
 * easiest path and quietly undid the point of the key.
 *
 * Not a dialog: dialogs here are for preferences, and this is the state the
 * whole application is in until it is answered. The key is kept in this
 * browser and sent as a header; the server never serves it, so there is
 * nothing here that can leak it.
 */
export function KeyGate() {
  const t = useT()
  const useApiKey = useStore((s) => s.useApiKey)
  const [key, setKey] = useState('')
  const [busy, setBusy] = useState(false)

  const submit = async (event: React.FormEvent) => {
    event.preventDefault()
    if (!key.trim() || busy) return
    setBusy(true)
    // A wrong key simply lands back here: `bootstrap` sets `needsKey` again.
    await useApiKey(key)
    setBusy(false)
  }

  return (
    <div className="gate">
      <form className="gate-card" onSubmit={submit}>
        <h1>{t('gate.title')}</h1>
        <p className="muted">{t('gate.hint')}</p>
        <input
          type="password"
          autoFocus
          value={key}
          placeholder={t('gate.placeholder')}
          onChange={(e) => setKey(e.target.value)}
          aria-label={t('gate.title')}
        />
        <Button tone="primary" type="submit" disabled={!key.trim() || busy}>
          {busy ? t('gate.checking') : t('gate.unlock')}
        </Button>
      </form>
    </div>
  )
}
