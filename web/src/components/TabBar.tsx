import { useT } from '../i18n'
import { useStore } from '../state/store'
import { TABS } from '../workspace/registry'
import { Icon, contextMenu } from '../ui'

/**
 * The workspace's tabs.
 *
 * Reading a paper, annotating it and asking about it are things done alongside
 * each other, so they get tabs rather than modals: a modal would take the
 * screen and lose its state the moment it closed.
 */
export function TabBar() {
  const t = useT()
  const tabs = useStore((s) => s.tabs)
  const active = useStore((s) => s.activeTab)
  const activateTab = useStore((s) => s.activateTab)
  const closeTab = useStore((s) => s.closeTab)
  const closeTabs = useStore((s) => s.closeTabs)

  // One tab is the workbench itself; a bar showing only that is chrome for
  // nothing, so it stays out of the way until there is a choice to make.
  if (tabs.length <= 1) return null

  return (
    <div className="tab-bar">
      {tabs.map((tab) => {
        const def = TABS[tab.kind]
        const Glyph = Icon[def.icon]
        return (
          <div
            key={tab.id}
            className="tab"
            data-active={tab.id === active}
            onMouseDown={(e) => {
              // Middle-click closes, as it does in every browser.
              if (e.button === 1) {
                e.preventDefault()
                closeTab(tab.id)
              } else if (e.button === 0) {
                activateTab(tab.id)
              }
            }}
            onContextMenu={contextMenu(() => [
              { label: t('tabs.close'), disabled: tab.permanent, onSelect: () => closeTab(tab.id) },
              { label: t('tabs.closeOthers'), onSelect: () => closeTabs('others', tab.id) },
              { label: t('tabs.closeAll'), onSelect: () => closeTabs('all') },
            ])}
            title={tab.title || t(def.labelKey)}
          >
            <Glyph size={12} className="tab-icon" />
            <span className="tab-label">{tab.title || t(def.labelKey)}</span>
            {!tab.permanent && (
              <button
                className="tab-close"
                title={t('tabs.close')}
                onMouseDown={(e) => e.stopPropagation()}
                onClick={() => closeTab(tab.id)}
              >
                <Icon.Close size={9} />
              </button>
            )}
          </div>
        )
      })}
    </div>
  )
}
