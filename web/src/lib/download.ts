/** Saving text the browser generated.
 *
 *  An export arrives as a string and has to become a file on somebody's disk.
 *  Kept apart from the menu that calls it so the awkward part — an object URL
 *  that leaks unless it is revoked — is written once and tested once.
 */
export function saveText(name: string, text: string, type = 'text/plain'): void {
  const url = URL.createObjectURL(new Blob([text], { type: `${type};charset=utf-8` }))
  const link = document.createElement('a')
  link.href = url
  link.download = name
  // Firefox will not follow a click on an element that is not in the document.
  document.body.append(link)
  link.click()
  link.remove()
  // The blob stays in memory until this happens, and a library export is not
  // small. Deferred because revoking it in the same tick cancels the download
  // in some browsers.
  setTimeout(() => URL.revokeObjectURL(url), 0)
}

/** What to call an exported file. */
export function exportName(format: string, count: number): string {
  const extension = format === 'bibtex' ? 'bib' : format === 'csljson' ? 'json' : 'ris'
  return `yinkote-${count}-items.${extension}`
}
