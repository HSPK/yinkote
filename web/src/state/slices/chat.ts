/** Conversations with the library agent.
 *
 *  Kept apart from the item list because a conversation is the user's own
 *  record rather than a view of the library: it survives navigation, it is not
 *  refetched when items change, and it is the one place where the answer may
 *  legitimately take twenty seconds to arrive.
 */
import type { StateCreator } from 'zustand'
import { failureOf } from '../../lib/errors'
import { displayTitle } from '../../lib/format'

import { api } from '../../api/client'
import type {
  AgentStatus,
  Conversation,
  Message,
  MessagePage,
  RunState,
} from '../../api/types'
import { tabId } from '../../lib/tabs'
import type { State } from '../store'

/** What a page of messages means for the store.
 *
 *  Written once and defensively: the same call is made from four places, and
 *  a response missing its `messages` array would otherwise crash the pane the
 *  moment it drew — which is how the run-state pane failed once already.
 */
function fromPage(page: MessagePage | undefined): {
  messages: Message[]
  hasOlder: boolean
} {
  return { messages: page?.messages ?? [], hasOlder: page?.hasMore ?? false }
}

export interface ChatSlice {
  conversations: Conversation[]
  /** Whether a model is configured; the chat box explains itself if not. */
  agent: AgentStatus | null
  /** A question is in flight. */
  asking: boolean
  conversation: string | null
  messages: Message[]
  /** Whether the thread has more above what is loaded. */
  hasOlder: boolean
  /** True while older messages are on their way. */
  loadingOlder: boolean
  /** Fetch the page before the oldest one held. */
  loadOlder: () => Promise<void>

  openConversation: (key: string, keep?: boolean) => Promise<void>
  /** The turn in flight for each conversation, by key. */
  runs: Record<string, RunState>
  /** Apply a state pushed by the server, or fetched on arrival. */
  applyRun: (conversation: string, state: unknown) => void
  /** Ask the current turn to stop at its next step. */
  cancelRun: () => Promise<void>
  newConversation: () => Promise<void>
  renameConversation: (key: string, title: string) => Promise<void>
  /** Point the current conversation at a collection, or clear it. */
  setConversationScope: (scope: string | null) => Promise<void>
  /** Point the assistant at a model. */
  configureAgent: (patch: {
    endpoint?: string
    model?: string
    apiKey?: string
    allowCommands?: boolean
    disabledSkills?: string[]
    disabledTools?: string[]
  }) => Promise<void>
  removeConversation: (key: string) => Promise<void>
  sendMessage: (text: string, mentions?: string[]) => Promise<void>
  askAbout: (itemKey: string) => Promise<void>
  /** Ask the last question again, after a turn that failed. */
  retry: () => Promise<void>
  summarise: (itemKey: string, language?: string) => Promise<boolean>
  /** Read one paper closely. Resolves false when the model was cut short. */
  closeReading: (itemKey: string, language?: string) => Promise<boolean>
}

export const createChatSlice: StateCreator<State, [], [], ChatSlice> = (set, get) => ({
  conversations: [],
  agent: null,
  asking: false,
  conversation: null,
  messages: [],
  hasOlder: false,
  loadingOlder: false,
  runs: {},

  applyRun(conversation, state) {
    if (!conversation) return
    const run = state as RunState
    const record = () => set({ runs: { ...get().runs, [conversation]: run } })

    // Still working, or a thread nobody is looking at: nothing to swap.
    if (run.running || get().conversation !== conversation) return record()

    // A finished turn's answer is stored as an ordinary message, so reload the
    // thread rather than trying to splice one in: the server knows the id.
    //
    // Fetch *before* dropping the live turn, and swap both in one write. The
    // obvious order — record the finish, then reload — takes the answer off
    // screen for a whole round trip and puts an identical one back, which is
    // the flicker at the end of every turn.
    void api.conversations
      .messages(get().library, conversation)
      .then((page) => {
        if (get().conversation !== conversation) return record()
        set({ runs: { ...get().runs, [conversation]: run }, ...fromPage(page) })
      })
      .catch(record)
  },

  async configureAgent(patch) {
    // The status the save returns is authoritative — re-fetching to find out
    // whether it worked is a race with the next reader.
    set({ agent: await api.configureAgent(patch) })
  },

  async setConversationScope(scope) {
    const s = get()
    if (!s.conversation) return
    const updated = await api.conversations.setScope(s.library, s.conversation, scope)
    set({
      conversations: s.conversations.map((c) => (c.key === updated.key ? updated : c)),
    })
  },

  /** Fetch the page before the oldest message held.
   *
   *  A conversation is read from the bottom, so the top is where "there is
   *  more" lives. Guarded against overlapping requests: scrolling fires more
   *  often than the network answers, and two in flight would insert the same
   *  page twice.
   */
  async loadOlder() {
    const s = get()
    const oldest = s.messages[0]?.id
    if (!s.conversation || !s.hasOlder || s.loadingOlder || oldest === undefined) return

    set({ loadingOlder: true })
    try {
      const page = await api.conversations.messages(s.library, s.conversation, {
        before: oldest,
      })
      // Re-read: the thread may have been switched while this was in flight.
      if (get().conversation !== s.conversation) return
      set({ messages: [...page.messages, ...get().messages], hasOlder: page.hasMore })
    } catch {
      set({ hasOlder: false })
    } finally {
      set({ loadingOlder: false })
    }
  },

  async cancelRun() {
    const key = get().conversation
    if (key) await api.conversations.cancel(get().library, key).catch(() => {})
  },

  /** Show a conversation.
   *
   *  As a glance, like a paper or a graph: reading through past threads is
   *  browsing, and the same gesture must not leave a tab behind for each one.
   *  Writing into it keeps it — see `sendMessage`. A freshly created thread is
   *  kept from the start, because asking for a new one *is* the intent. */
  async openConversation(key, keep = false) {
    const title = get().conversations.find((c) => c.key === key)?.title
    set({ conversation: key })
    get().openTab({
      id: tabId('chat', key),
      kind: 'chat',
      title: title || '',
      target: key,
      preview: !keep,
    })
    try {
      set(fromPage(await api.conversations.messages(get().library, key)))
    } catch {
      set({ messages: [] })
    }

    // Rejoin whatever is already happening. Without this a reload during a
    // long turn shows an idle conversation while the model is still working.
    const run = await api.conversations.run(get().library, key).catch(() => null)
    if (run) get().applyRun(key, run)
  },

  async newConversation() {
    const created = await api.conversations.create(get().library)
    set({ conversations: [created, ...get().conversations] })
    await get().openConversation(created.key, true)
  },

  async renameConversation(key, title) {
    await api.conversations.rename(get().library, key, title)
    set({ conversations: await api.conversations.list(get().library) })
  },

  async removeConversation(key) {
    await api.conversations.remove(get().library, key)
    get().closeTab(tabId('chat', key))
    if (get().conversation === key) set({ conversation: null, messages: [] })
    set({ conversations: await api.conversations.list(get().library) })
  },

  /** Ask the agent, or just record the turn when no model is configured.
   *
   *  Either way the question is persisted: a transcript that loses what was
   *  typed because a model was unreachable would be worse than no transcript. */
  async sendMessage(text, mentions = []) {
    const s = get()
    const body = text.trim()
    if (!body || !s.conversation || s.asking) return

    const optimistic: Message = {
      id: -Date.now(),
      role: 'user',
      content: body,
      mentions,
      createdAt: Date.now(),
    }
    set({ messages: [...s.messages, optimistic], asking: true })
    // Typing into a surface is the clearest statement that it is not a glance.
    get().keepTab(tabId('chat', s.conversation))

    try {
      if (s.agent?.configured) {
        // Returns as soon as the turn exists; the answer arrives on the event
        // bus. Awaiting it here would tie a half-minute of work to one request
        // and lose it on a reload.
        await api.conversations.ask(s.library, s.conversation, body, mentions)
        const run = await api.conversations.run(s.library, s.conversation).catch(() => null)
        if (run) get().applyRun(s.conversation, run)
      } else {
        // Mentions travel on this path too. Without a model the workbench is
        // still a place to keep a thread against a paper, and dropping what
        // the user attached would lose it silently.
        await api.conversations.append(s.library, s.conversation, {
          role: 'user',
          content: body,
          mentions,
        })
      }
      set(fromPage(await api.conversations.messages(s.library, s.conversation)))
    } catch (e) {
      set({ error: failureOf(e) })
      if (s.conversation) {
        set(fromPage(await api.conversations.messages(s.library, s.conversation)))
      }
    } finally {
      set({ asking: false })
    }

    // Name an untitled thread from its opening line.
    const current = s.conversations.find((c) => c.key === s.conversation)
    if (current && current.messageCount === 0) {
      const title = body.length > 40 ? `${body.slice(0, 40)}…` : body
      await get().renameConversation(s.conversation, title)
    } else {
      set({ conversations: await api.conversations.list(s.library) })
    }
  },

  /** Start a conversation about one item, seeded with what it is.
   *
   *  The agent could look the item up itself, but it would first have to guess
   *  which one was meant — and the caller already knows. */
  async askAbout(itemKey) {
    const item = get().items.find((i) => i.key === itemKey)
    if (!item) return
    // Asking about a highlight named the thread after its key. Its own words
    // are the only label it has, and they are what the thread is about.
    const title = displayTitle(item, itemKey)

    const created = await api.conversations.create(get().library, { title })
    set({ conversations: [created, ...get().conversations] })
    await get().openConversation(created.key)
    await get().sendMessage(
      `About the item "${title}" (key ${itemKey}) in my library. Use get_item to read it first.`,
    )
  },

  /** Ask the model for a summary; it lands as a note under the item.
   *
   *  Returns whether the model ran out of steps. The answer used to be thrown
   *  away, so a summary that stops mid-thought was announced as "Summary
   *  added" like any other. */
  /** Ask the last question again.
   *
   *  A failed turn told the reader "the model is busy, try again in a moment"
   *  and gave them nothing to try again *with* -- they had to retype the
   *  question, and a long one typed into a box that just lost it is a question
   *  people abandon. The text is still in the thread; this sends it again. */
  async retry() {
    const s = get()
    if (!s.conversation || s.asking) return
    // The last thing the reader said, which is what the failed turn was for.
    const asked = [...s.messages].reverse().find((m) => m.role === 'user')
    if (!asked?.content) return
    await get().sendMessage(asked.content, asked.mentions ?? [])
  },

  async summarise(itemKey, language) {
    const { truncated } = await api.summarise(get().library, itemKey, language)
    await get().refresh()
    return truncated
  },

  async closeReading(itemKey, language) {
    const { truncated } = await api.closeReading(get().library, itemKey, language)
    await get().refresh()
    return truncated
  },
})
