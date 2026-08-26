import type { ComponentType } from 'react'

import type { MessageKey } from '../i18n'
import type { TabKind } from '../lib/tabs'
import { ChatView } from '../pages/ChatView'
import { CollectionsPage } from '../pages/CollectionsPage'
import { ItemTable } from '../components/ItemTable'
import { PluginsPage } from '../pages/PluginsPage'
import { ReaderView } from '../pages/ReaderView'
import { StatusPage } from '../pages/StatusPage'
import type { IconName } from '../ui'

export interface TabDefinition {
  Body: ComponentType<{ target?: string }>
  icon: IconName
  /** Used when a tab has no title of its own. */
  labelKey: MessageKey
  /** Whether the detail pane is useful beside this surface. */
  withDetail?: boolean
}

/**
 * What each kind of tab is.
 *
 * The one place a surface is registered: adding a workspace view means an entry
 * here and a component, with nothing in the shell to change. That is also the
 * seam a plugin-contributed view would slot into.
 */
export const TABS: Record<TabKind, TabDefinition> = {
  library: { Body: ItemTable, icon: 'Library', labelKey: 'nav.library', withDetail: true },
  collections: { Body: CollectionsPage, icon: 'Folder', labelKey: 'nav.collections' },
  chat: { Body: ChatView, icon: 'Chat', labelKey: 'sidebar.chat' },
  reader: { Body: ReaderView, icon: 'Library', labelKey: 'reader.title', withDetail: true },
  plugins: { Body: PluginsPage, icon: 'Plugin', labelKey: 'nav.plugins' },
  status: { Body: StatusPage, icon: 'Gauge', labelKey: 'nav.status' },
}
