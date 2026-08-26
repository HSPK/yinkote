import { useT } from '../i18n'
import { Empty, Section } from '../ui'

/** Conversation workspace.
 *
 *  The transport and tool loop land with the agent backend; this page owns the
 *  layout so the two can be developed independently.
 */
export function ChatPage() {
  const t = useT()
  return (
    <div className="page narrow">
      <Section title={t('chat.title')}>
        <Empty>
          <p>{t('chat.disabled')}</p>
          <p className="muted">{t('chat.hint')}</p>
        </Empty>
      </Section>
    </div>
  )
}
