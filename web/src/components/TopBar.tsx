import { useStore } from '../state/store'
import type { SearchMode } from '../api/types'

const MODES: { id: SearchMode; label: string; hint: string }[] = [
  { id: 'hybrid', label: 'hyb', hint: '混合：关键词 + 模糊 + 语义融合' },
  { id: 'keyword', label: 'key', hint: '关键词：BM25 精确匹配' },
  { id: 'fuzzy', label: 'fuz', hint: '模糊：容错拼写与部分词' },
  { id: 'semantic', label: 'sem', hint: '语义：向量近邻' },
]

export function TopBar() {
  const query = useStore((s) => s.query)
  const mode = useStore((s) => s.mode)
  const setQuery = useStore((s) => s.setQuery)
  const setMode = useStore((s) => s.setMode)

  return (
    <header className="topbar">
      <div className="brand">
        YINKOTE<small>workbench</small>
      </div>

      <div className="search">
        <input
          id="search-input"
          value={query}
          spellCheck={false}
          autoComplete="off"
          placeholder="搜索标题 / 作者 / 摘要 …  支持 tag:综述  type:book  year:2020..2024  -tag:废弃"
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Escape') {
              setQuery('')
              e.currentTarget.blur()
            }
          }}
        />
        <div className="modes">
          {MODES.map((m) => (
            <button
              key={m.id}
              title={m.hint}
              data-active={mode === m.id}
              onClick={() => setMode(m.id)}
            >
              {m.label}
            </button>
          ))}
        </div>
      </div>

      <span className="kbd-hint">/ 搜索 · ⌘K 命令 · J/K 移动 · Del 删除</span>
    </header>
  )
}
