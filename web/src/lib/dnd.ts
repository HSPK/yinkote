/** Typed drag payloads.
 *
 *  The DataTransfer API is stringly-typed and, during `dragover`, refuses to
 *  reveal its contents at all — so a drop target cannot ask "what is this?"
 *  while deciding whether to accept. We therefore keep the payload in a module
 *  variable for the duration of the drag and use the DataTransfer only to make
 *  the gesture look native and to carry a text fallback for outside apps.
 */

export type DragPayload =
  | { kind: 'items'; keys: string[] }
  | { kind: 'collection'; key: string }

const MIME = 'application/x-yinkote'

let active: DragPayload | null = null

export function beginDrag(e: React.DragEvent, payload: DragPayload, label: string): void {
  active = payload
  e.dataTransfer.effectAllowed = payload.kind === 'collection' ? 'move' : 'copyMove'
  e.dataTransfer.setData(MIME, JSON.stringify(payload))
  e.dataTransfer.setData('text/plain', label)
}

export function endDrag(): void {
  active = null
}

/** What is currently being dragged, or `null` outside a drag. */
export function dragging(): DragPayload | null {
  return active
}

/** The payload if it is of the wanted kind, else `null`. Lets a target say
 *  `accepts('items')` and get a typed answer in one call. */
export function accepts<K extends DragPayload['kind']>(
  kind: K,
): Extract<DragPayload, { kind: K }> | null {
  return active?.kind === kind ? (active as Extract<DragPayload, { kind: K }>) : null
}

/** Reads the payload out of a drop event, falling back to the module state.
 *  The event is authoritative because a drag may cross frames. */
export function readDrop(e: React.DragEvent): DragPayload | null {
  const raw = e.dataTransfer.getData(MIME)
  if (!raw) return active
  try {
    return JSON.parse(raw) as DragPayload
  } catch {
    return active
  }
}
