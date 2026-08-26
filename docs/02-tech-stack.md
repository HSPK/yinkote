# 02 · 技术栈与选型

## 1. 决策总表

| 层 | 选型 | 版本/说明 |
| --- | --- | --- |
| 核心服务语言 | **Rust** | axum + tokio + tower |
| Web 框架 | **axum** | REST + WebSocket + 静态资源；`tower-http` 提供压缩/CORS/限流 |
| 数据库 | **SQLite（WAL）** | `sqlx`（编译期 SQL 校验）或 `rusqlite`；迁移用 `sqlx migrate` |
| 全文检索 | **Tantivy**（+ `lindera` CJK 分词） | MVP 可先用 SQLite FTS5 过渡 |
| 向量检索 | **`sqlite-vec` / `usearch`** | V1.x 语义检索；embedding 用 `fastembed-rs` 本地模型 |
| JS 运行时 | **`rquickjs`（QuickJS）** | 沙箱执行 translators 与 citeproc-js |
| PDF | **`pdfium-render`**（渲染/抽文本）+ **pdf.js**（前端阅读器） | 避开 MuPDF 的 AGPL |
| 引文渲染 | **citeproc-js on QuickJS**，长期评估 `citeproc-rs` | 保证与 Zotero 输出一致 |
| 桌面壳 | **Tauri v2** | 托盘、自启、自动更新、单实例、原生对话框 |
| **Agent 运行时** | **`@earendil-works/pi-coding-agent`（Node/TS sidecar）** | 自定义工具 + 只读文件工具 + JSONL 会话树；打包为单文件可执行，按需下载 |
| **图计算** | `petgraph` + 自研 Louvain / ForceAtlas2 | 服务端预计算 PageRank、社区、坐标 |
| **图可视化** | `graphology` + `sigma.js`（WebGL） | 万级节点流畅渲染；D3 力导向在 2k 节点即卡 |
| 前端框架 | **React 19 + TypeScript 5** | Vite 构建 |
| 路由/状态 | TanStack Router + TanStack Query + Zustand | 服务端状态与 UI 状态分离 |
| UI | Tailwind CSS + shadcn/ui + Radix | 可控、可主题化 |
| 大列表 | TanStack Virtual | 10 万条目虚拟滚动 |
| 富文本笔记 | **TipTap（ProseMirror）** | 结构化、可扩展 `@` 引用与标注引用节点 |
| 拖拽 | dnd-kit | 条目拖入收藏夹 |
| 浏览器扩展 | **WXT** + MV3 + React | 一套代码出 Chrome/Edge/Firefox |
| Word 插件 | **Office.js 加载项**（任务窗格）+ React | Win/Mac/Web 版 Word 通用 |
| API 契约 | **OpenAPI 3.1**（`utoipa` 从 Rust 生成）→ TS SDK | 单一事实来源 |
| 测试 | Rust: `cargo test` + `insta` 快照；前端: Vitest + Testing Library；E2E: Playwright | |
| 可观测 | `tracing` + `tracing-subscriber`（JSON 日志） + 本地 `/api/v1/debug/metrics` | 默认不外传任何数据 |
| CI/CD | GitHub Actions 矩阵构建（win/mac-x64/mac-arm/linux）+ Tauri Updater | |

## 2. 核心语言选型：为什么是 Rust

打分（1-5，加权）：

| 维度 | 权重 | Rust | Node/TS | Go | Python |
| --- | --- | --- | --- | --- | --- |
| 单文件分发 / 安装无依赖 | 高 | 5 | 3 | 5 | 1 |
| 常驻内存与启动速度 | 高 | 5 | 2 | 4 | 2 |
| 与 Tauri 桌面壳协同 | 中 | 5 | 3 | 2 | 1 |
| 文献生态复用（translators/citeproc/pdf.js 都是 JS） | 高 | 3* | 5 | 2 | 3 |
| 检索引擎（Tantivy） | 中 | 5 | 3 | 4 | 3 |
| AI/NLP 生态（GROBID、embedding） | 中 | 3 | 3 | 2 | 5 |
| 团队上手速度 | 中 | 2 | 5 | 4 | 5 |
| **加权结论** | | **最高** | 次高 | 中 | 低 |

\* Rust 的生态短板通过**内嵌 QuickJS** 补齐：translators 与 citeproc-js 本就是纯 JS，放在 `rquickjs` 沙箱里跑，既复用生态又不引入 Node 运行时依赖。

### 备选路线（写清楚退路）

- **路线 B：Node/TypeScript（Fastify）全栈。** 优点：与 Zotero 生态（translation-server、citeproc-js、pdf.js）零摩擦，全栈同语言，人才好招，开发速度最快。缺点：打包需 SEA/pkg（体积 60–100MB）、常驻内存高、原生模块跨平台编译麻烦。**若团队 JS 背景为主，选它并不丢人**，架构分层不变。
- **路线 C：Go。** 二进制小、并发好，但 `goja` 跑 translators 兼容性风险高，PDF 生态弱。
- **路线 D：Python（FastAPI）。** AI/解析生态最强（GROBID、PyMuPDF、AnyStyle），但"给普通用户装一个后台服务"的分发体验最差。
- **混合方案（推荐的现实妥协）：** Rust 主进程 + **可选** Python sidecar 处理重 AI/解析任务（用户按需下载"AI 增强包"）。

## 3. QuickJS 沙箱：复用 JS 生态的关键

```
┌──────────────── yk-translate ────────────────┐
│  rquickjs Context (每次调用新建，超时 10s)    │
│  注入的宿主 API：                             │
│    Zotero.Utilities.*   (xpath/trim/clean…)  │
│    Zotero.Item(type)    → 回调到 Rust 建对象   │
│    ZU.HTTP.request()    → Rust reqwest 代理    │
│    doc (DOM)            → 由 Rust 侧解析 HTML  │
│  限制：无 fs、无 net（必须走宿主代理）、内存上限 │
└──────────────────────────────────────────────┘
```

要点：
- **DOM 供给**：用 Rust 的 `scraper`/`html5ever` 解析 HTML，把只读 DOM 以 JS 对象形式暴露（`querySelector`、`evaluate`/XPath 子集）。这是工作量最大的一块，需要按 translators 实际用到的 API 面逐步补齐。
- **网络代理**：JS 不直连网络，所有请求经 Rust 侧执行，可统一做超时、UA、Cookie、代理、限速、可审计。
- **降级方案**：若某站点 translator 依赖过多浏览器 API，则**在浏览器扩展的内容脚本中执行**（那里有真 DOM），只把结果 JSON 回传服务端。这是浏览器扩展相对服务端抓取的天然优势。
- **兜底方案**：MVP 阶段先只做「服务端 HTTP 抓取 + 自研解析器（arXiv/Crossref/OpenAlex/PubMed/DOI/知网）」，translator 运行时作为第二阶段。

## 4. Agent 运行时为什么单独开一个 Node 进程

这看似违背了"单进程、零外部依赖"的原则，但权衡后是正确的：

| 论点 | 说明 |
| --- | --- |
| pi-coding-agent 是 TypeScript SDK | 用 Rust 重写一个同等成熟度的 agent harness（工具循环、上下文压缩、重试、会话树、多模型适配）至少要 6 个月，且长期维护成本高 |
| Agent 是**可选**能力 | 不装 = 主程序体积与内存不受任何影响；核心文献管理功能 100% 可用 |
| 进程隔离反而是优点 | Agent 会跑不可信内容（网页、PDF 里的提示词注入）、会 OOM、会卡在网络 IO —— 隔离在子进程里崩了不影响主服务 |
| 安全边界更清晰 | `agentd` 只有一个受限 scope 的 API Key，与第三方插件同等待遇，可审计、可吊销 |
| 打包可控 | Bun `build --compile` 或 Node SEA 产出单文件（40–60MB），随「AI 增强包」下载，不进主安装包 |
| 有语言无关退路 | pi 提供 `--mode rpc`，若将来换语言写核心，协议不变 |

> 反面选项（放弃过）：① 在 QuickJS 里跑 pi —— pi 依赖 Node API，不可行；② 自研 Rust agent 循环 —— 重复造轮子且能力落后；③ 把整个后端换成 Node —— 见 §2 的分发与内存代价。

## 5. 前端关键技术点

| 关注点 | 方案 |
| --- | --- |
| 三栏布局 | 收藏夹树 / 条目表格 / 条目详情，可拖拽分栏，布局持久化到本地 |
| 大数据量表格 | TanStack Table + Virtual，服务端分页 + 游标；列宽/排序持久化 |
| PDF 阅读器 | pdf.js（`pdfjs-dist`）+ 自研标注层（SVG overlay），支持高亮/下划线/区域/墨迹/批注 |
| EPUB 阅读器 | `epub.js`，V1.x |
| 离线与 PWA | Service Worker 缓存 App Shell；数据层用 TanStack Query 持久化缓存；写操作离线排队 |
| 实时协作（未来） | 笔记用 Yjs CRDT，通过 WS 同步；条目元数据仍走版本号乐观锁 |
| i18n | `i18next`，首发 zh-CN / en-US |
| 快捷键 | 全局命令面板（Cmd+K）：搜索、跳转、执行动作 |

## 6. 依赖清单（核心）

**Rust**
```
axum, tower-http, tokio, hyper
sqlx (sqlite, macros, migrate)
serde / serde_json, utoipa (+ utoipa-swagger-ui)
tantivy, lindera-tantivy, sqlite-vec, fastembed
rquickjs
pdfium-render
petgraph                       # 图算法
reqwest (rustls-tls), scraper, html5ever
argon2, rand, jsonwebtoken 或 tower-sessions
tracing, tracing-subscriber, anyhow/thiserror
notify (监听 linked-file 目录), walkdir, blake3/md-5
rcgen (本地 HTTPS 自签证书)
handlebars 或 minijinja        # 存储路径模板
jsonschema                     # 元数据校验
```

**前端**
```
react, react-dom, typescript, vite
@tanstack/{react-router,react-query,react-table,react-virtual}
zustand, immer
tailwindcss, class-variance-authority, radix-ui, lucide-react
@tiptap/react + extensions
pdfjs-dist
graphology, graphology-layout-forceatlas2, sigma       # 关系图谱
i18next, react-i18next
zod（运行时校验 API 响应）
```

**agentd（Node/TS）**
```
@earendil-works/pi-coding-agent   # Agent harness
@earendil-works/pi-ai             # 模型与凭据
typebox                           # 工具参数 schema
@yinkote/api-client               # 本仓库生成的 REST SDK
```

## 7. 版本与兼容策略

- API 走 `/api/v1`，破坏性变更升 `v2` 并保留 `v1` 至少两个大版本。
- 数据库迁移**只前进**（forward-only），启动时自动执行；迁移前自动做一次 `yinkote.db` 快照备份到 `backups/`。
- 客户端（插件）用 `GET /api/v1/ping` 返回的 `apiVersion`/`minClientVersion` 做兼容性协商，不匹配时在 UI 明确提示升级。
