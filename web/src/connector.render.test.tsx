/**
 * Turning browser saving on and off.
 *
 * This section used to be a readout: it said the connector was off and named a
 * command-line flag as the way to change that — on a product whose recommended
 * install is a background service, where nobody types a command line. Quick add
 * then began advising people to use the connector when a publisher refuses us,
 * which made the dead end worse: advice pointing at an unreachable feature.
 *
 * So these tests are about the section being a *control*, and about it telling
 * the truth when the switch does not work — binding can fail, because a running
 * Zotero owns port 23119.
 */
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { BrowserConnector } from './components/BrowserConnector'
import type { ConnectorStatus } from './api/types'
import { enUS } from './i18n/en-US'
import { useOverlays } from './ui/overlays'

const asked: (number | null)[] = []
let reply: ConnectorStatus | Error = { state: 'listening', port: 23119 }

vi.mock('./api/client', () => ({
  api: {
    connector: {
      set: (port: number | null) => {
        asked.push(port)
        return reply instanceof Error ? Promise.reject(reply) : Promise.resolve(reply)
      },
    },
  },
  connectEvents: () => () => {},
}))

let root: Root
let host: HTMLElement

beforeEach(() => {
  asked.length = 0
  useOverlays.setState({ toasts: [] })
  host = document.createElement('div')
  document.body.append(host)
})

afterEach(() => {
  act(() => root.unmount())
  host.remove()
})

function render(status: ConnectorStatus, onChange = () => {}) {
  act(() => {
    root = createRoot(host)
    root.render(<BrowserConnector status={status} onChange={onChange} />)
  })
}

const button = () =>
  [...host.querySelectorAll('button')].find((b) =>
    [enUS['connector.turnOn'], enUS['connector.turnOff']].includes(b.textContent ?? ''),
  )

describe('the connector section is a switch, not a readout', () => {
  it('offers to turn it on when it is off', async () => {
    render({ state: 'off' })
    const b = button()
    expect(b?.textContent).toBe(enUS['connector.turnOn'])

    await act(async () => b!.click())
    // 23119 is not ours to choose: it is the port the Zotero extensions look
    // for, so offering a free choice here would just be a way to get it wrong.
    expect(asked).toEqual([23119])
  })

  it('offers to turn it off once it is listening', async () => {
    reply = { state: 'off' }
    render({ state: 'listening', port: 23119 })
    expect(button()?.textContent).toBe(enUS['connector.turnOff'])

    await act(async () => button()!.click())
    // Turning off must clear the port, not re-send it.
    expect(asked).toEqual([null])
  })

  it('tells the caller so the badge cannot go stale', async () => {
    let refreshed = 0
    render({ state: 'off' }, () => {
      refreshed += 1
    })
    await act(async () => button()!.click())
    expect(refreshed).toBe(1)
    expect(useOverlays.getState().toasts.at(-1)?.tone).toBe('success')
  })

  it('says so when the port is already taken', async () => {
    // The failure that actually happens: a running Zotero owns 23119. A switch
    // that silently did nothing here would be the worst possible outcome.
    reply = new Error('conflict: could not listen on 127.0.0.1:23119')
    let refreshed = 0
    render({ state: 'off' }, () => {
      refreshed += 1
    })
    await act(async () => button()!.click())

    // Asserted on the toast store, not the DOM: this component is rendered on
    // its own here, so the overlay host that paints toasts is not in the tree.
    const toasts = useOverlays.getState().toasts
    expect(toasts.map((t) => t.message)).toContain(enUS['connector.failed'])
    expect(toasts.at(-1)?.tone).toBe('error')
    // Nothing changed, so nothing should claim it did.
    expect(refreshed).toBe(0)
  })
})
