/** Conversations with the library agent.
 *
 *  Kept apart from the item list because a conversation is the user's own
 *  record rather than a view of the library: it survives navigation, it is not
 *  refetched when items change, and it is the one place where the answer may
 *  legitimately take twenty seconds to arrive.
 */
import type { StateCreator } from 'zustand'

import { api } from '../../api/client'
import type { AgentStatus, Conversation, Message } from '../../api/types'
import { tabId } from '../../lib/tabs'
import type { State } from '../store'

export interface ChatSlice {
  conversations: Conversation[]
  /** Whether a model is configured; the chat box explains itself if not. */
  agent: AgentStatus | null
  /** A question is in flight. */
  asking: boolean
  conversation: string | null
  messages: Message[]

  openConversation: (key: string, keep?: boolean) => Promise<void>
  newConversation: () => Promise<void>
  renameConversation: (key: string, title: string) => Promise<void>
  removeConversation: (key: string) => Promise<void>
  sendMessage: (text: string) => Promise<void>
  askAbout: (itemKey: string) => Promise<void>
  summarise: (itemKey: string) => Promise<void>
}

export const createChatSlice: StateCreator<State, [], [], ChatSlice> = (set, get) => ({
  conversations: [],
  agent: null,
  asking: false,
  conversation: null,
  messages: [],

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
      set({ messages: await api.conversations.messages(get().library, key) })
    } catch {
      set({ messages: [] })
    }
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
  async sendMessage(text) {
    const s = get()
    const body = text.trim()
    if (!body || !s.conversation || s.asking) return

    const optimistic: Message = {
      id: -Date.now(),
      role: 'user',
      content: body,
      createdAt: Date.now(),
    }
    set({ messages: [...s.messages, optimistic], asking: true })
    // Typing into a surface is the clearest statement that it is not a glance.
    get().keepTab(tabId('chat', s.conversation))

    try {
      if (s.agent?.configured) {
        await api.conversations.ask(s.library, s.conversation, body)
      } else {
        await api.conversations.append(s.library, s.conversation, { role: 'user', content: body })
      }
      // Re-read rather than splice: the server may have appended several
      // messages, and it is the one that knows their ids.
      set({ messages: await api.conversations.messages(s.library, s.conversation) })
    } catch (e) {
      set({ error: e instanceof Error ? e.message : String(e) })
      if (s.conversation) {
        set({ messages: await api.conversations.messages(s.library, s.conversation) })
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
    const title = String(item.title ?? itemKey)

    const created = await api.conversations.create(get().library, { title })
    set({ conversations: [created, ...get().conversations] })
    await get().openConversation(created.key)
    await get().sendMessage(
      `About the item "${title}" (key ${itemKey}) in my library. Use get_item to read it first.`,
    )
  },

  /** Ask the model for a summary; it lands as a note under the item. */
  async summarise(itemKey) {
    await api.summarise(get().library, itemKey)
    await get().refresh()
  },
})
