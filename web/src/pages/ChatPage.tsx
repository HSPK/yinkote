import { Empty, Section } from '../ui'

/** Conversation workspace.
 *
 *  The transport and tool loop land with the agent backend; this page owns the
 *  layout so the two can be developed independently.
 */
export function ChatPage() {
  return (
    <div className="page narrow">
      <Section title="文库对话">
        <Empty>
          <p>对话式检索与问答尚未启用。</p>
          <p className="muted">
            届时可以直接问「这个收藏夹里的方法怎么分类」或「帮我找扩散模型做分子生成的近三年工作」，
            答案会带可点击的条目引用。
          </p>
        </Empty>
      </Section>
    </div>
  )
}
