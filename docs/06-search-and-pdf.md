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

### 2.1 服务端（`yk-pdf`）

**已实现**：纯 Rust 文本层抽取（`pdf-extract`，MIT/Apache），无外部依赖、无模型、离线可用。真实论文实测 65,370 字符 / 89ms，39,315 字符 / 128ms。

原计划用 pdfium。改用纯 Rust 的理由是**单一二进制**这个前提：pdfium 需要预编译的平台原生库，跨平台分发要为每个目标准备一份 `.so`/`.dylib`/`.dll`，而这个产品的承诺是下载一个文件就能跑。带坐标抽取与位图渲染尚未实现——目前没有功能需要它们，标注坐标由前端 pdf.js 提供。

**实测的能力边界**（不是猜测）：

| | 表现 |
| --- | --- |
| 正文段落 | 很好。摘要与精读所依赖的就是正文，跨行断词会重新拼接，分栏空白会折叠 |
| 表格 | 会摊平成若干行，列对应关系丢失。模型引用表内数字时可能张冠李戴 |
| 上标 | `10²⁰` 变成 `10 20`，指数与底数分离 |
| 扫描件 | 完全读不出。由 `is_useful()` 判定（按字母数字字符计数，不是 `is_empty()`——扫描页会产出零星杂字符） |

### 2.1.1 为什么不内置深度学习模型

Marker / MinerU / PaddleOCR 在表格、公式、阅读顺序上确实更好。不内置的理由不是效果，是**代价与前提冲突**：

- 它们是 Python 程序，权重 1–2 GB，要 GPU 才快；本产品是用户下载即用的单一二进制。
- 它们解决的是少数文件（扫描件、重表格论文）。为少数情况让所有人付出安装 Python + 下载模型 + 占用显存的代价，是用产品前提去换一部分文件上的更好答案。
- 自研版面分析模型更不成立：那是一个独立的研究项目，且已有成熟开源方案。

所以做成**接缝而非依赖**（`yk_pdf::Pipeline`）：任何"接受文件路径、把文本打到 stdout"的程序都可以在配置里挂上，内置读取器始终是默认值，也是外部程序未安装或失败时的兜底。

```toml
[pdf]
mode = "fallback"        # off（默认）| fallback（仅当读不出文本层时）| always
command = "marker_single"
args = ["{}", "--output_format", "markdown"]
timeout_secs = 300
```

- `{}` 是文件路径占位符——不写会让程序在空文件上"成功"，因此启动即拒绝。
- 外部程序缺失、超时或失败**永远不会成为答案**：记一条日志，回退到内置结果。这是每台新机器的初始状态。
- `fallback` 是推荐值：版面模型慢，而它唯一不可替代的场景（扫描件）恰好是内置读取器完全无能为力的那个。

OCR（Tesseract / PaddleOCR）通过同一个接缝接入，不需要另一套机制。

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
