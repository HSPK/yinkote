/** Putting text on the clipboard, wherever the workbench is being used from.
 *
 *  `navigator.clipboard` only exists in a secure context: HTTPS, or localhost.
 *  The server binds to the whole machine so a phone or a second computer can
 *  reach it, and over `http://192.168.x.x` the API is simply *not there* —
 *  reading `.writeText` off it threw "Cannot read properties of undefined",
 *  which surfaced as "Could not render the citation" and blamed the renderer
 *  for something the browser had decided before it ran.
 *
 *  So: the real API when it exists, and the old selection-and-execCommand
 *  trick when it does not. That trick is deprecated and still works
 *  everywhere, which is exactly what a fallback is for.
 */

/** Whether the browser will let us copy at all. */
export function canCopy(): boolean {
  return typeof navigator !== 'undefined' || typeof document !== 'undefined'
}

export async function copyText(text: string): Promise<void> {
  const clipboard = typeof navigator === 'undefined' ? undefined : navigator.clipboard
  if (clipboard?.writeText) {
    try {
      await clipboard.writeText(text)
      return
    } catch {
      // Permission refused, or the document was not focused. The fallback
      // below does not need permission, so it is worth trying rather than
      // reporting a failure the user can do nothing about.
    }
  }

  if (typeof document === 'undefined' || !document.body) {
    throw new Error('clipboard.unavailable')
  }

  // Off-screen rather than hidden: a `display: none` element cannot be
  // selected, and an unselected one copies nothing.
  const area = document.createElement('textarea')
  area.value = text
  area.setAttribute('readonly', '')
  area.style.position = 'fixed'
  area.style.top = '-1000px'
  area.style.opacity = '0'
  document.body.append(area)

  const selection = document.getSelection()
  const previous = selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null

  try {
    area.select()
    area.setSelectionRange(0, text.length)
    if (!document.execCommand('copy')) throw new Error('clipboard.unavailable')
  } finally {
    area.remove()
    // Put back whatever the reader had selected; copying a citation should not
    // clear the sentence they were reading.
    if (previous && selection) {
      selection.removeAllRanges()
      selection.addRange(previous)
    }
  }
}
