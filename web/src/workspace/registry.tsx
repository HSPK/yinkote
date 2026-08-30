import type { ComponentType } from 'react'

import type { MessageKey } from '../i18n'
import type { TabKind } from '../lib/tabs'
import { ChatView } from '../pages/ChatView'
import { ChatsPage } from '../pages/ChatsPage'
import { CollectionsPage } from '../pages/CollectionsPage'
import { DownloadsPage } from '../pages/DownloadsPage'
import { FilesPage } from '../pages/FilesPage'
import { DuplicatesPage } from '../pages/DuplicatesPage'
import { TasksPage } from '../pages/TasksPage'
import { GapsPage } from '../pages/GapsPage'
import { GraphView } from '../pages/GraphView'
import { ItemTable } from '../components/ItemTable'
import { PluginsPage } from '../pages/PluginsPage'
import { SettingsPage } from '../pages/SettingsPage'
import { CollectionEditorHost } from '../components/CollectionEditorHost'
import { NoteView } from '../pages/NoteView'
import { ReaderView } from '../pages/ReaderView'
import { StatusPage } from '../pages/StatusPage'
import type { IconName } from '../ui'
import { CollectionDetail } from '../components/CollectionDetail'
import { ConversationDetail } from '../components/ConversationDetail'
import {
  ChatFooter,
  ChatsFooter,
  CollectionsFooter,
  GapsFooter,
  GraphFooter,
  LibraryFooter,
  ReaderFooter,
} from './footers'

export interface TabDefinition {
  Body: ComponentType<{ target?: string }>
  icon: IconName
  /** Used when a tab has no title of its own. */
  labelKey: MessageKey
  /** Whether the detail pane is useful beside this surface. */
  withDetail?: boolean
  /** What this surface contributes to the status bar. */
  Footer?: ComponentType
  /** What the detail pane shows here; the item inspector when unset. */
  Detail?: ComponentType
  /** What the toolbar's search box does here. */
  search?: 'items' | 'collections' | 'find' | 'none'
}

/**
 * What each kind of tab is.
 *
 * The one place a surface is registered: adding a workspace view means an entry
 * here and a component, with nothing in the shell to change. That is also the
 * seam a plugin-contributed view would slot into.
 */
export const TABS: Record<TabKind, TabDefinition> = {
  library: {
    Body: ItemTable,
    icon: 'Library',
    labelKey: 'nav.library',
    withDetail: true,
    Footer: LibraryFooter,
    search: 'items',
  },
  collections: {
    Body: CollectionsPage,
    icon: 'Folder',
    labelKey: 'nav.collections',
    withDetail: true,
    Footer: CollectionsFooter,
    Detail: CollectionDetail,
    search: 'collections',
  },
  chat: { Body: ChatView, icon: 'Chat', labelKey: 'sidebar.chat', Footer: ChatFooter, search: 'none' },
  chats: {
    Body: ChatsPage,
    icon: 'Chat',
    labelKey: 'chats.title',
    withDetail: true,
    Detail: ConversationDetail,
    Footer: ChatsFooter,
    // Searching filters the list in place, as it does for collections.
    search: 'collections',
  },
  note: {
    Body: NoteView,
    icon: 'Note',
    labelKey: 'note.title',
    withDetail: true,
    search: 'none',
  },
  reader: {
    Body: ReaderView,
    icon: 'Library',
    labelKey: 'reader.title',
    withDetail: true,
    Footer: ReaderFooter,
    search: 'find',
  },
  graph: {
    Body: GraphView,
    icon: 'Graph',
    labelKey: 'graph.title',
    withDetail: true,
    Footer: GraphFooter,
    search: 'none',
  },
  tasks: {
    Body: TasksPage,
    icon: 'Gauge',
    labelKey: 'tasks.title',
    search: 'none',
  },
  duplicates: {
    Body: DuplicatesPage,
    icon: 'Library',
    labelKey: 'duplicates.title',
    search: 'none',
  },
  gaps: {
    Body: GapsPage,
    icon: 'Graph',
    labelKey: 'gaps.title',
    Footer: GapsFooter,
    search: 'none',
  },
  files: {
    Body: FilesPage,
    icon: 'Library',
    labelKey: 'files.title',
    withDetail: true,
    search: 'none',
  },
  downloads: {
    Body: DownloadsPage,
    icon: 'Download',
    labelKey: 'downloads.title',
    search: 'none',
  },
  plugins: { Body: PluginsPage, icon: 'Plugin', labelKey: 'nav.plugins', search: 'none' },
  status: { Body: StatusPage, icon: 'Gauge', labelKey: 'nav.status', search: 'none' },
  settings: { Body: SettingsPage, icon: 'Settings', labelKey: 'nav.settings', search: 'none' },
  'collection-edit': {
    Body: CollectionEditorHost,
    icon: 'Folder',
    labelKey: 'collection.edit',
    search: 'none',
  },
}
