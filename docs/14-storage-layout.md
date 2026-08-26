# 14 · 存储布局与元数据规范

> 本文细化 [03-data-model](03-data-model.md) 与 [05-storage-sync](05-storage-sync.md)，是"数据可读、可迁移、可自主掌控"的落地规范。

## 1. 设计原则

1. **人可读优先**：打开文件管理器就能看懂目录结构，不是一堆随机字符串。
2. **数据库可重建**：`yinkote.db` 丢了也能从磁盘上的文件 + sidecar 元数据重建出 95% 的库（`index/`、`cache/` 更是随时可删）。
3. **路径由用户定义**：文件放哪、叫什么名字，由模板决定，随时可改并安全重组织。
4. **元数据是纯文本**：JSON，可 `git` 管理、可 `grep`、可被任何工具读取。
5. **不锁死用户**：任何时候可一键导出全库为标准格式；卸载 Yinkote 后文献依然可用。

## 2. 数据目录总览

```
$YINKOTE_DATA/                          # 可在设置里整体迁移
├─ yinkote.db                           # ⭐ 权威索引（SQLite）
├─ config.toml                           
├─ library/                             # ⭐ 人可读的文献库（附件 + sidecar 元数据）
│  └─ …见 §3
├─ metadata/                            # ⭐ 可选：全量元数据镜像（每条目一个 JSON，便于 git）
│  └─ items/A1/A1B2C3D4.json
├─ notes/                               # 可选：笔记以 Markdown 镜像输出
├─ agents/                               
│  ├─ sessions/                         # pi JSONL 会话
│  ├─ skills/                           # 用户自定义检索 Skill
│  └─ auth.json                         # 模型凭据（权限 600）
├─ agent-workspace/                     # Agent 只读文件视图（cache 级，可删）
├─ index/                               # Tantivy 索引（可删，可重建）
├─ vectors/                             # 向量索引（可删，可重建）
├─ cache/                               # 缩略图、渲染页、翻译缓存（可删）
├─ styles/  locales/                    # 用户安装的 CSL 样式与语言包
├─ backups/                             # 自动 DB 快照
└─ logs/
```

**三层数据分级**（决定备份与同步策略）：

| 级别 | 目录 | 丢失后果 | 备份 |
| --- | --- | --- | --- |
| 🔴 权威 | `yinkote.db`、`library/`、`metadata/`、`agents/sessions/` | 数据丢失 | 必须 |
| 🟡 可重算但昂贵 | `index/`、`vectors/`、`fulltext`（在 db 内） | 需重跑几十分钟 | 建议 |
| 🟢 纯缓存 | `cache/`、`agent-workspace/` | 无 | 不必 |

## 3. 附件路径模板

### 3.1 默认布局

```
library/
├─ 机器学习/                                    ← {{collection}}
│  ├─ Vaswani_2017_Attention Is All You Need/   ← {{firstAuthor}}_{{year}}_{{title}}
│  │  ├─ Vaswani_2017_Attention Is All You Need.pdf
│  │  ├─ supplementary.pdf
│  │  ├─ snapshot.html
│  │  └─ .yinkote.json                          ← ⭐ sidecar 元数据
│  └─ Ho_2020_Denoising Diffusion Probabilistic Models/
│     └─ …
└─ _未分类/
```

### 3.2 模板语法

```toml
[storage]
# 目录模板：决定条目文件夹放在哪
folder_template = "{{collection|first|default:_未分类}}/{{firstAuthor}}_{{year}}_{{title|truncate:60}}"
# 文件名模板：决定主附件叫什么
file_template   = "{{firstAuthor}}_{{year}}_{{title|truncate:60}}"
# 冲突时追加：_2, _3 …
collision       = "suffix"        # suffix | key | error
# 非法字符替换
sanitize        = "strict"        # strict(仅 ASCII+CJK) | loose
max_path_len    = 200
# 布局风格预设
preset          = "by-collection" # by-collection | by-author | by-year | by-venue | flat | key-based
```

| 变量 | 示例 |
| --- | --- |
| `{{itemKey}}` | `A1B2C3D4` |
| `{{firstAuthor}}` / `{{authors}}` | `Vaswani` / `Vaswani et al.` |
| `{{year}}` `{{date}}` | `2017` |
| `{{title}}` | 原标题 |
| `{{itemType}}` | `journalArticle` |
| `{{venue}}` `{{publisher}}` | `NeurIPS` |
| `{{collection}}` | 所属收藏夹（多个时取 `first`） |
| `{{project}}` | 所属项目 |
| `{{tag:主题}}` | 指定命名空间的标签值 |
| `{{doi}}` `{{arxivId}}` | 标识符 |
| `{{citekey}}` | BibTeX citekey（与 `.bib` 导出一致） |

过滤器：`truncate:N` `lower` `upper` `slug` `pinyin`（中文转拼音，跨平台文件名友好）`first` `default:X` `pad:4`。

### 3.3 预设对比

| 预设 | 结构 | 适合 |
| --- | --- | --- |
| `by-collection`（默认） | `收藏夹/作者_年份_标题/` | 大多数人 |
| `by-author` | `作者/年份_标题/` | 追踪特定学者 |
| `by-year` | `2024/作者_标题/` | 按时间归档 |
| `by-venue` | `NeurIPS/2017/…` | 会议/期刊导向 |
| `flat` | `作者_年份_标题.pdf`（无子目录） | 与 Zotfile 习惯一致 |
| `key-based` | `A1B2C3D4/paper.pdf` | **最稳健**：路径永不变，重命名零成本；牺牲可读性 |

> ⚠️ **权衡说明（必须写进 UI 帮助）**：模板化路径可读性好，但条目元数据改变（改标题、换收藏夹）会引发文件移动。Yinkote 的做法是 —— **DB 始终是权威**，路径只是"投影"；移动失败不会丢数据，只会标记 `path_dirty` 待修复。追求绝对稳健的用户可选 `key-based`。

### 3.4 重组织引擎

改模板 / 改预设 / 批量改元数据后：

```
POST /api/v1/maintenance/reorganize/plan
     { folderTemplate?, fileTemplate?, scope? }
  → { moves: [{itemKey, from, to}], conflicts: [...], count: 1284 }   # 只算不动
POST /api/v1/maintenance/reorganize/apply   { planId, dryRun: false }
  → 事务式执行：逐条 rename → 更新 DB → 写入 undo 日志
POST /api/v1/maintenance/reorganize/undo    { planId }
```
- **先预览后执行**，冲突（同名、路径超长、跨盘）在预览阶段暴露。
- 执行期间写 `reorganize_journal` 表，中断后可续跑或回滚。
- 使用 rename（同盘原子）而非 copy+delete；跨盘时 copy → 校验哈希 → 删除。

## 4. Sidecar 元数据 `.yinkote.json`

每个条目文件夹里放一份**自包含**的元数据，让目录脱离 Yinkote 也有意义：

```jsonc
{
  "$schema": "https://yinkote.dev/schema/item-v1.json",
  "yinkoteVersion": "1.0.0",
  "key": "A1B2C3D4",
  "libraryKey": "L001",
  "version": 128,
  "item": {                       // ← 与 items.data 完全一致的规范结构
    "itemType": "journalArticle",
    "title": "Attention Is All You Need",
    "creators": [{ "creatorType": "author", "firstName": "Ashish", "lastName": "Vaswani" }],
    "date": "2017-06-12",
    "DOI": "10.48550/arXiv.1706.03762",
    "publicationTitle": "NeurIPS",
    "abstractNote": "…",
    "tags": [{ "tag": "transformer", "type": 0 }],
    "extra": "Citation Key: vaswani2017attention"
  },
  "csl": { /* 派生的 CSL-JSON，方便第三方直接用 */ },
  "collections": ["机器学习/Transformer"],
  "attachments": [
    { "filename": "Vaswani_2017_Attention Is All You Need.pdf",
      "contentType": "application/pdf", "hash": "blake3:9f2a…", "size": 2214578,
      "linkMode": "imported_url", "url": "https://arxiv.org/pdf/1706.03762" }
  ],
  "annotationsFile": "annotations.json",   // 标注单独一份，避免主文件过大
  "notesFiles": ["notes/思路整理.md"],
  "dateAdded": "2026-08-26T09:00:00Z",
  "dateModified": "2026-08-26T09:10:00Z"
}
```

- **写入时机**：条目变更后防抖 2s 异步写；批量导入时合并写。可在设置里关闭（`storage.sidecar = false`）以减少 IO。
- **冲突处理**：以 DB 为权威。检测到 sidecar 的 `version` 比 DB 新（例如从别处同步来的），进冲突队列，不静默覆盖。
- **灾难恢复**：`yinkote recover --from library/` 扫描全部 sidecar 重建 DB（重建后需重跑索引）。

## 5. 元数据规范

### 5.1 单一事实来源：`resources/schema/`

```
resources/schema/
├─ item-types.json          # 条目类型 → 字段列表、字段顺序、创建者类型
├─ fields.json              # 字段 → 类型(text|date|number)、别名、i18n 标签
├─ csl-mapping.json         # 条目类型/字段 ↔ CSL 类型/变量 的双向映射
├─ bibtex-mapping.json      # ↔ BibTeX
├─ ris-mapping.json         # ↔ RIS
└─ item-v1.schema.json      # JSON Schema，用于校验 items.data 与 sidecar
```
Rust 与 TypeScript **共用同一份 JSON**（Rust 用 `include_str!` 编译期嵌入并生成类型，TS 用 `json-schema-to-typescript`）。新增字段只改 JSON，前后端同时生效，且自动获得校验能力。

### 5.2 校验

- 写入前用 JSON Schema 校验；失败返回 `422` 并指明字段路径。
- 未知字段不丢弃 —— 收进 `extra` 或 `_unmapped`，保证导入的稀有格式无损往返。
- 日期统一存 EDTF 风格字符串（`2017`、`2017-06`、`2017-06-12`、`2017-06/2017-07`），同时派生 `year` 整数供排序。
- 中文姓名：`fieldMode: 1`（单字段）或双字段，导入时按语言与格式启发式判断，用户可批量修正。

### 5.3 稳定标识符

| 标识符 | 稳定性 | 用途 |
| --- | --- | --- |
| `items.id` | 仅本机 | 内部外键 |
| `items.key`（8 位） | **跨设备、跨导出永久稳定** | API、引用、sidecar、同步 |
| `citekey` | 由规则生成，可变 | BibTeX / LaTeX |
| `yinkote://items/A1B2C3D4` | 永久 | 笔记内链、Word 引文、深链 |

## 6. 从 Zotero 导入（细化）

```
POST /api/v1/import/zotero
{
  "dataDir": "/Users/x/Zotero",
  "mode": "copy",                    // copy | link（link 不复制附件，直接引用原目录）
  "applyStorageTemplate": true,      // 导入时即按新模板重排文件名/目录
  "notesFormat": "keep-html",        // keep-html | to-markdown
  "importAnnotations": true,
  "importGroups": true,
  "dryRun": false
}
```

### 迁移映射表

| Zotero | Yinkote | 备注 |
| --- | --- | --- |
| `items` / `itemData` / `itemDataValues` / `fields` | `items.data` + `item_fields` | 按 `csl-mapping` 校验，未知字段进 `extra` |
| `creators` / `itemCreators` | `item_creators` | 保留 `fieldMode`（中文姓名/机构） |
| `collections` / `collectionItems` | `collections` / `collection_items` | 保留层级与顺序 |
| `tags` / `itemTags` | `tags` / `item_tags` | 保留彩色标签与位置 |
| `itemNotes` | `items(itemType=note)` | HTML 保留；可选转 Markdown |
| `itemAttachments` + `storage/<key>/` | `attachments` + `library/…` | **保留原 key**，按模板重命名 |
| `itemAnnotations`（Zotero 6+） | `annotations` | position/sortIndex 格式兼容，直接搬 |
| `savedSearches` | `saved_searches` | 条件语法逐条映射，不支持的标注出来 |
| `relations`（`dc:relation`, `owl:sameAs`） | `relations` | 额外写入 `owl:sameAs zotero://select/…` 便于回溯 |
| `libraries`（群组库） | `libraries(type=group)` | 只读导入，权限需重新设置 |
| Better BibTeX 的 citekey（`extra` 或 BBT 数据库） | `citekey` | ⭐ 保留，避免 LaTeX 用户的引用全部失效 |

### 安全保证

- **源库只读**：先把 `zotero.sqlite` 复制到临时目录再打开（Zotero 运行时会锁库）。
- **可预演**：`dryRun: true` 输出完整报告（条目数、类型分布、字段丢失清单、附件缺失清单），不动任何数据。
- **可回滚**：导入在一个新 `library` 里进行，失败可整库删除，不影响已有数据。
- **报告留档**：`logs/import-zotero-<时间戳>.json`，含每条被跳过/降级的记录。

## 7. 导出与"离开的自由"

| 导出 | 内容 |
| --- | --- |
| `POST /maintenance/export-all` | `.yinkote` 整包（zip：db + library + metadata + notes + manifest） |
| BibTeX / RIS / CSL-JSON / EndNote XML | 标准交换格式 |
| Zotero RDF | 回流到 Zotero |
| Markdown 库 | 每条目一个 `.md`（YAML front-matter + 笔记 + 标注），可直接进 Obsidian |
| GraphML / GEXF | 关系图谱 |
| 纯目录 | `library/` 本身就是可用的成果：PDF + 可读文件名 + JSON 元数据 |

> 产品承诺写进 README：**"你的文献库首先是你磁盘上的文件夹，其次才是 Yinkote 的数据库。"**
