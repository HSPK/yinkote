import { useEffect, useRef, useState } from 'react'

import { api } from '../api/client'
import type { Item } from '../api/types'
import { t } from '../i18n'
import { failureOf, failureText, type Failure } from '../lib/errors'
import { toast } from '../ui/overlays'

const AUTOSAVE_MS = 1200
type Fields = { title: string; body: string }
interface DraftState extends Fields {
  note: Item | null
  loading: boolean
  saving: boolean
  saved: boolean
  dirty: boolean
  error: Failure | null
}

const empty = (): DraftState => ({
  note: null, title: '', body: '', loading: true,
  saving: false, saved: false, dirty: false, error: null,
})
const fieldsOf = (note: Item): Fields => ({
  title: String(note.title ?? ''), body: String(note.note ?? ''),
})

// A closing editor can still be saving when the same note is reopened.
// Share that queue, and retain failed drafts until they can be recovered.
const drafts = new Map<string, NoteDraft>()

class NoteDraft {
  state = empty()
  stored: Fields = { title: '', body: '' }
  listeners = new Set<() => void>()
  onSaved?: (note: Item) => void
  timer?: ReturnType<typeof setTimeout>
  loading?: Promise<void>
  saving?: Promise<boolean>

  constructor(readonly library: number, readonly key: string) {}

  get id() { return `${this.library}:${this.key}` }

  publish() {
    this.state = {
      ...this.state,
      dirty: !!this.state.note && (
        this.state.title !== this.stored.title || this.state.body !== this.stored.body
      ),
    }
    this.listeners.forEach((notify) => notify())
  }

  release() {
    if (!this.listeners.size && !this.loading && !this.saving && !this.state.dirty) {
      if (drafts.get(this.id) === this) drafts.delete(this.id)
    }
  }

  fail(error: unknown, saving = true) {
    this.state.error = failureOf(error)
    this.state.saved = false
    if (saving && !this.listeners.size) {
      toast.error(t('note.saveFailed'), failureText(t, this.state.error))
    }
  }

  load() {
    if (this.loading) return this.loading
    this.state.loading = true
    this.state.error = null
    this.publish()
    this.loading = (async () => {
      try {
        const note = await api.items.get(this.library, this.key)
        this.stored = fieldsOf(note)
        this.state = { ...this.state, ...this.stored, note }
      } catch (error) {
        this.fail(error, false)
      } finally {
        this.state.loading = false
        this.loading = undefined
        this.publish()
        this.release()
      }
    })()
    return this.loading
  }

  edit(field: keyof Fields, value: string) {
    if (!this.state.note) return
    this.state[field] = value
    this.state.saved = false
    this.publish()
    clearTimeout(this.timer)
    if (!this.state.error) this.timer = setTimeout(() => void this.flush(), AUTOSAVE_MS)
  }

  flush(retry = false): Promise<boolean> {
    clearTimeout(this.timer)
    if (this.saving) return this.saving
    if (!this.state.note) return Promise.resolve(true)
    if (!this.state.dirty) return Promise.resolve(true)
    if (this.state.error && !retry) return Promise.resolve(false)
    const rebase = retry && this.state.error?.code === 'version_conflict'
    this.state.error = null
    this.state.saving = true
    this.publish()
    this.saving = (async () => {
      try {
        if (rebase) {
          // Explicit "Save my changes": take the latest version, preserving
          // remote fields that this editor did not change.
          const latest = await api.items.get(this.library, this.key)
          const remote = fieldsOf(latest)
          for (const field of ['title', 'body'] as const) {
            if (this.state[field] === this.stored[field]) this.state[field] = remote[field]
          }
          this.stored = remote
          this.state.note = latest
          this.publish()
        }
        while (this.state.dirty) {
          const sent = { title: this.state.title, body: this.state.body }
          const fields: Record<string, string> = {}
          if (sent.title !== this.stored.title) fields.title = sent.title
          if (sent.body !== this.stored.body) fields.note = sent.body
          const updated = await api.items.update(
            this.library, this.key, { fields }, this.state.note!.version,
          )
          this.stored = fieldsOf(updated)
          // A response acknowledges the sent snapshot, not a newer keystroke.
          for (const field of ['title', 'body'] as const) {
            if (this.state[field] === sent[field]) this.state[field] = this.stored[field]
          }
          this.state.note = updated
          this.publish()
          this.onSaved?.(updated)
        }
        this.state.saved = true
        return true
      } catch (error) {
        this.fail(error)
        return false
      } finally {
        this.state.saving = false
        this.saving = undefined
        this.publish()
        this.release()
      }
    })()
    return this.saving
  }
}

export function useNoteDraft(
  library: number,
  target?: string,
  onSaved?: (note: Item) => void,
) {
  const [owned, setOwned] = useState<{ session: NoteDraft; state: DraftState } | null>(null)
  const current = useRef<NoteDraft | null>(null)

  useEffect(() => {
    if (!target) {
      current.current = null
      setOwned(null)
      return
    }
    const id = `${library}:${target}`
    const session = drafts.get(id) ?? new NoteDraft(library, target)
    drafts.set(id, session)
    current.current = session
    const notify = () => setOwned({ session, state: session.state })
    session.listeners.add(notify)
    notify()
    if (!session.state.note && !session.state.error) void session.load()
    const beforeUnload = (event: BeforeUnloadEvent) => {
      if (!session.state.dirty && !session.state.saving) return
      void session.flush()
      event.preventDefault()
      event.returnValue = ''
    }
    window.addEventListener('beforeunload', beforeUnload)
    return () => {
      session.listeners.delete(notify)
      if (current.current === session) current.current = null
      void session.flush()
      session.release()
      window.removeEventListener('beforeunload', beforeUnload)
    }
  }, [library, target])

  useEffect(() => {
    if (current.current) current.current.onSaved = onSaved
  }, [library, target, onSaved])

  const session = owned?.session.library === library && owned.session.key === target
    ? owned.session
    : null
  return {
    ...(session ? owned!.state : empty()),
    edit: (field: keyof Fields, value: string) => session?.edit(field, value),
    flush: async () => {
      const saved = await session?.flush()
      // A slow Back must not navigate away from a newly selected note.
      return !!saved && current.current === session
    },
    retry: () => {
      if (!session) return
      if (!session.state.note) void session.load()
      else void session.flush(true)
    },
  }
}
