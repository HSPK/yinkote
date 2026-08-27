import type { ComponentType } from 'react'

import type { MessageKey } from '../i18n'
import type { TabKind } from '../lib/tabs'
import { ChatView } from '../pages/ChatView'
import { CollectionsPage } from '../pages/CollectionsPage'
import { GapsPage } from '../pages/GapsPage'
import { GraphView } from '../pages/GraphView'
import { ItemTable } from '../components/ItemTable'
import { PluginsPage } from '../pages/PluginsPage'
import { ReaderView } from '../pages/ReaderView'
import { StatusPage } from '../pages/StatusPage'
import type { IconName } from '../ui'
import { CollectionDetail } from '../components/CollectionDetail'
import {
  ChatFooter,
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
  gaps: {
    Body: GapsPage,
    icon: 'Graph',
    labelKey: 'gaps.title',
    Footer: GapsFooter,
    search: 'none',
  },
  plugins: { Body: PluginsPage, icon: 'Plugin', labelKey: 'nav.plugins', search: 'none' },
  status: { Body: StatusPage, icon: 'Gauge', labelKey: 'nav.status', search: 'none' },
}
