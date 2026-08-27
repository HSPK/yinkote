/** A value that lags behind, so work keyed on it happens once.
 *
 *  Walking a list with the arrow keys changes the selection twenty times a
 *  second. Anything the detail panel fetches for the selected row would then
 *  be fetched twenty times a second too — each request cheap, all of them
 *  competing with the list query for the same connection, and every answer
 *  but the last one thrown away.
 *
 *  The same reasoning as the search box, which has debounced since the first
 *  version: a keystroke is not a request.
 */
import { useEffect, useState } from 'react'

/** Long enough to skip the rows passed through, short enough to feel instant. */
export const DETAIL_DEBOUNCE_MS = 120

export function useDebounced<T>(value: T, ms = DETAIL_DEBOUNCE_MS): T {
  const [settled, setSettled] = useState(value)

  useEffect(() => {
    // First arrival is not a change: waiting to show anything at all would
    // make opening a paper feel slower than it is.
    if (settled === value) return
    const timer = window.setTimeout(() => setSettled(value), ms)
    return () => window.clearTimeout(timer)
  }, [value, ms, settled])

  return settled
}
