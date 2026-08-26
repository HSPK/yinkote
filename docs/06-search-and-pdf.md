# 06 · 检索、PDF 与 AI

## 1. 检索体系

三层检索，UI 上统一入口但内部分流：

| 层 | 数据源 | 实现 | 延迟目标 |
| --- | --- | --- | --- |
| L1 字段检索 | `item_fields` / `item_creators` / `tags` | SQL + 索引 | < 20ms |
| L2 全文检索 | 附件抽出的正文、笔记、标注、摘要 | Tantivy | p95 < 200ms |
| L3 语义检索 | chunk 级 embedding | sqlite-vec / usearch | < 500ms |

搜索框输入 → 同时发 L1 + L2，结果按类型分组（"条目 / 全文命中 / 笔记 / 标注"），L3 作为"语义相关"折叠区（V1.x）。

### 1.1 中文分词（关键差异化）

- SQLite FTS5 默认 `unicode61` 对中文按整段切，几乎不可用；`trigram` 勉强可用但精度差、索引大。
- **选型：Tantivy + `lindera`（IPADIC/CC-CEDICT 词典）**，同时支持中日韩；英文用标准 tokenizer + 词干还原。
- 混合语料策略：一个 `text` 字段配 **多分词器多字段**（`text.zh`、`text.en`），查询时对两者并行打分取最大值，避免中英混排论文召回下降。
- 索引 schema：
  ```
  doc { item_id(u64, stored), library_id(u64, indexed),
        kind(str: item|note|annotation|fulltext),
        title(text.zh+en, boost 3), creators(text, boost 2),
        abstract(text), body(text), tags(text),
        year(u64 fast), date_modified(u64 fast) }
  ```
- 支持高亮片段（Tantivy snippet）、短语查询、前缀查询、布尔与字段限定（`title:transformer AND year:>2020`）。

### 1.2 索引生命周期

- 写事务提交 → 领域事件 → 索引任务入队（批量 commit，默认 500ms 或 200 篇合并一次，减少 segment 碎片）。
- 附件正文抽取是慢操作，独立任务：`pdf_extract` → 写 `fulltext` 表 → 再进索引。
- `index/` 可整体删除，`POST /maintenance/reindex` 从 DB + `fulltext` 表全量重建（不需要重新解析 PDF）。
- MVP 若时间紧：先用 **FTS5 + `simple` 分词器扩展**（支持中文和拼音首字母），接口层抽象为 `trait SearchEngine`，后续无痛切 Tantivy。

## 2. PDF 处理

### 2.1 服务端（`yk-pdf`，基于 pdfium）

| 能力 | 用途 |
| --- | --- |
| 文本抽取（带坐标） | 全文索引、标注文本回填、元数据识别 |
| 页面渲染为位图 | 封面缩略图、区域标注截图、无 JS 环境预览 |
| 文档信息 | 页数、页标签（罗马数字前言页）、大纲、内嵌元数据 |
| 内嵌注释读写 | 导入 PDF 自带高亮；导出"带批注的 PDF" |
| OCR（可选） | 扫描件：Tesseract sidecar 或 PaddleOCR（中文），按需下载 |

选 pdfium 而非 MuPDF 的原因：**许可证**（pdfium 为 BSD 3-Clause，MuPDF 为 AGPL/商业双授权），见 10-licensing。

### 2.2 前端阅读器

基于 `pdfjs-dist` 自建阅读器，而不是内嵌第三方成品，理由是标注层要与我们的数据模型深度绑定。

```
┌ 阅读器组件树 ────────────────────────────────┐
│ <ReaderShell>                                │
│  ├ <Sidebar>  缩略图 | 大纲 | 标注列表 | 搜索 │
│  ├ <Viewer>                                  │
│  │   ├ pdf.js canvas 层（页面渲染）           │
│  │   ├ text 层（可选中，pdf.js 提供）         │
│  │   ├ annotation 层（SVG，自绘高亮/墨迹）    │
│  │   └ selection popup（高亮/下划线/复制引文）│
│  └ <NotePanel> 右侧笔记，双向定位              │
└──────────────────────────────────────────────┘
```

- **坐标模型**：`{ pageIndex, rects: [[x1,y1,x2,y2]] }` 基于 PDF 用户空间（不随缩放变化），与 Zotero 标注格式保持互通，便于导入导出。
- **排序键 `sortIndex`**：`页码(5位)|字符偏移(6位)|Y(5位)` 拼串，字典序即阅读顺序，使标注列表、生成笔记的顺序天然正确。
- **性能**：页面虚拟化（仅渲染视口 ±2 页）、渲染任务队列可取消、缩略图 Web Worker 生成、大 PDF 通过 HTTP Range 按需取字节。
- **协作/多端**：标注写入即 PATCH，WS 广播，另一台设备打开同一 PDF 实时看到。

### 2.3 元数据识别（拖入 PDF 自动认领）

流水线（命中即停）：
1. PDF 内嵌 XMP / DOI 元数据
2. 首页正则扫 DOI / arXiv ID / ISBN
3. 提取标题候选（最大字号文本块）→ Crossref/OpenAlex 标题查询 → 打分匹配
4. 全文哈希 → 学术搜索引擎查（可选）
5. 失败 → 标记 `needs_metadata`，UI 提示手工补全

## 3. AI 能力（V1.x，全部可关闭、可换 Provider）

### 3.1 Provider 抽象

```rust
trait LlmProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatStream>;
}
trait EmbeddingProvider {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
}
```
内置实现：**OpenAI 兼容 API**（覆盖 OpenAI / DeepSeek / Qwen / Kimi / 自建 vLLM）、**Ollama**（本地）、**fastembed-rs**（本地纯 Rust embedding，无需外部服务）。

配置在 `settings`，密钥加密存储（OS keychain 优先，回落到本地加密文件）。**默认全部关闭**，开启时明确告知"内容将发送到 <endpoint>"。

### 3.2 功能清单

| 功能 | 说明 |
| --- | --- |
| 摘要与要点 | 对 PDF 正文分块 → map-reduce 摘要，结果落为一条自动笔记（可编辑） |
| 自动标签 | 从摘要+关键词生成 `type=1` 自动标签，与手动标签区分，可一键撤销 |
| 翻译 | 段落级 / 全文对照翻译，缓存到 `cache/translations/` |
| Chat with PDF | 检索增强：向量召回 + BM25 混合（RRF 融合）→ 带页码引用作答，点击跳转原文 |
| 跨库问答 | 在选中的收藏夹范围内做 RAG，答案附条目引用 |
| 参考文献解析 | 粘贴一段参考文献文本 → 结构化条目（LLM + 规则双通道，交叉校验） |
| 综述辅助 | 对一组条目生成对比表格（方法/数据集/指标），导出 Markdown |

### 3.3 RAG 数据结构

```sql
CREATE TABLE chunks (
  id INTEGER PRIMARY KEY,
  attachment_id INTEGER NOT NULL,
  ordinal INTEGER NOT NULL,
  page_start INTEGER, page_end INTEGER,
  text TEXT NOT NULL,
  token_count INTEGER
);
-- 向量列用 sqlite-vec 虚拟表
CREATE VIRTUAL TABLE chunk_vec USING vec0(chunk_id INTEGER PRIMARY KEY, embedding FLOAT[768]);
```
分块策略：按 PDF 结构（标题层级）优先，回落到 512 token 滑窗、overlap 64。检索用 `vec` top-50 + Tantivy top-50 → RRF 融合 → rerank（可选本地 bge-reranker）→ top-8 入上下文。

## 4. 引文渲染（citeproc）

- 引擎：**citeproc-js 跑在 QuickJS**（与 Zotero 输出逐字一致，是"可信引用"的硬要求）；持续评估 `citeproc-rs` 作为替代以去掉 JS 依赖。
- 样式：内置 ~30 个常用样式（APA 7、MLA 9、Chicago、IEEE、Nature、Science、**GB/T 7714-2015（含中英混排规则）**、GB/T 7714-2005），其余从 CSL 官方仓库按需下载并缓存。
- 语言包：CSL locales，中文条目自动用 `zh-CN` locale（"等" 而非 "et al."）。
- 缓存：`(styleId, locale, itemVersion)` → 渲染结果 LRU 缓存，列表页批量渲染只算增量。
- 中文特殊需求：
  - 中英文混排参考文献分区排序（中文在前、英文在后）
  - `[J]` `[M]` `[D]` 文献类型标识
  - 作者三人以上 "等/et al." 的中英差异
  - 这些通过定制 CSL 样式 + `zh-CN` locale 覆盖实现，不 hack 引擎。
