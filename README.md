# Yinkote（引可特）

> 一个**本地优先（local-first）、Web 优先**的开源文献管理工具，Zotero 的现代化网页版替代。

用户在自己的电脑（或 NAS / 服务器）上安装并启动 Yinkote 后台服务，之后：

- **所有文献管理操作都在浏览器里完成**；
- **Word / WPS 插件**、**浏览器扩展**都通过同一套本地 **API Server** 与后台通信；
- 数据 100% 存在本地（SQLite + 附件目录），可选加密同步到自建 WebDAV / S3 / 私有服务器。

---

## 快速开始

```bash
# 1. 构建（Rust 1.85+ / Node 20+）
cargo build --release -p yk-server
(cd web && npm install && npm run build)

# 2. 启动
./target/release/yinkote --web-dir web/dist --plugin-dir plugins

# 3. 打开 http://127.0.0.1:23130
```

常用参数：

| 参数 | 说明 |
| --- | --- |
| `-p, --port <PORT>` | 监听端口，默认 `23130` |
| `--host <HOST>` | 绑定地址，默认 `127.0.0.1`（仅本机） |
| `--data-dir <DIR>` | 数据目录，默认平台标准位置 |
| `--web-dir <DIR>` | 工作台静态资源目录 |
| `--plugin-dir <DIR>` | 额外插件目录（可重复） |

环境变量：`YK_DATA_DIR` `YK_HOST` `YK_PORT` `YK_WEB_DIR` `YK_API_KEY` `YK_LOG`
`YK_EMBED_ENDPOINT` `YK_EMBED_MODEL` `YK_EMBED_API_KEY` `YK_EMBED_DIM`

### 开发

```bash
cargo run -p yk-server -- --data-dir ./.dev-data       # 后端
(cd web && npm run dev)                                # 前端 5273 端口，自动代理 /api

cargo test --workspace                                 # 156 个后端测试
cargo clippy --workspace --all-targets -- -D warnings
(cd web && npm test)                                   # 56 个前端测试
bash scripts/smoke.sh http://127.0.0.1:23130           # 30 项 API 冒烟
node scripts/bench.mjs http://127.0.0.1:23130 100000   # 10 万条目基准
```

---

## 已实现（v0.1）

| 领域 | 能力 |
| --- | --- |
| **条目管理** | schema 驱动的 19 种文献类型、批量写入（逐条隔离失败）、乐观锁、软删除 / 回收站 / 永久删除 + 墓碑 |
| **组织** | 无限层级收藏夹（防环）、标签（含自动标签、合并重命名）、分面统计 |
| **搜索** | 关键词 (BM25) · 模糊 (三元组 + 编辑距离) · 语义 (向量) · 标签/字段过滤，四路 **RRF 融合**；查询语言 `tag: type: author: year:2020..2024 -tag:x "精确短语"`；中文原生可搜 |
| **插件系统** | 清单发现、能力/权限模型、进程 JSON-RPC 2.0 运行时、双向宿主回调、钩子总线、超时/崩溃自愈/热重载/停用记忆 |
| **实时** | WebSocket 变更推送，前端增量刷新 |
| **界面** | 工业风格三栏工作台，虚拟滚动、命令面板 (⌘K)、键盘优先 |
| **运维** | 版本化增量同步基础、WAL 检查点、索引重建、统计面板 |

## 性能（10 万条目，单机 release 构建）

| 操作 | p50 | p95 |
| --- | --- | --- |
| 列表首页 | 8.1 ms | 10.1 ms |
| 按标题排序 | 8.1 ms | 8.7 ms |
| 关键词搜索 | 11.5 ms | 12.3 ms |
| 中文关键词 | 33.7 ms | 36.1 ms |
| 模糊搜索（含错拼） | 5.7 ms | 6.3 ms |
| 语义搜索 | 6.4 ms | 9.1 ms |
| 标签筛选 | 22.8 ms | 24.6 ms |
| 标签分面（缓存命中） | 1.9 ms | 2.4 ms |
| 混合搜索 | 35.8 ms | 37.5 ms |
| 单条写入 | 3.0 ms | 19.7 ms |
| 批量写入吞吐 | — | ~3000 条/秒 |

10 万条目全量向量化后常驻内存约 100 MB（256 维 × 4 字节）。

---

## 架构

```
crates/
├─ yk-core     领域模型、端口 (trait)、错误、事件、插件契约、条目 schema
├─ yk-store    SQLite 适配器：仓储、迁移、FTS/三元组/向量队列的事务内维护
├─ yk-search   混合检索：BM25 + 模糊 + 向量 + RRF 融合，嵌入提供方抽象
├─ yk-plugin   插件运行时：清单发现、进程 JSON-RPC、钩子总线、生命周期
└─ yk-server   HTTP/WS 接口、宿主回调 API、后台任务、工作台托管
web/           React + TypeScript 工作台
plugins/       示例插件（见 plugins/README.md）
```

依赖方向严格向内：`yk-server → yk-{store,search,plugin} → yk-core`。
所有跨层协作都经由 `yk-core::ports` 中的 trait，因此检索引擎、插件运行时、
嵌入提供方都可以整体替换而不触碰调用方。

### 几个关键设计

- **单一写入路径维护派生数据**：FTS5、三元组索引、嵌入队列由存储层在**同一个事务**里更新，
  索引不可能与数据漂移。
- **`BEGIN IMMEDIATE`**：延迟事务在并发写入下无法升级锁，SQLite 会立即返回
  `SQLITE_BUSY` 而不等待 busy_timeout。见 `crates/yk-store/tests/concurrency.rs`。
- **`CROSS JOIN` 固定查询计划**：普通 `JOIN` 会让 SQLite 从 `items` 驱动检索，
  10 万条目下 5 ms 变 18 s。见 `crates/yk-search/tests/query_plan.rs`。
- **后台任务让出写锁**：嵌入工作线程按 200 行分批提交并主动 yield，
  否则会饿死交互写入。
- **插件无特权**：插件只能通过权限受限的宿主 API 访问数据，与第三方客户端同等待遇。

---

## 文档

| 文档 | 内容 |
| --- | --- |
| [00-overview](docs/00-overview.md) | 产品定位、范围与非目标、核心用户旅程 |
| [01-architecture](docs/01-architecture.md) | 系统架构、进程模型、模块划分 |
| [02-tech-stack](docs/02-tech-stack.md) | 技术选型决策与备选路线 |
| [03-data-model](docs/03-data-model.md) | 数据模型、Schema、版本与软删除 |
| [04-api-design](docs/04-api-design.md) | REST / WebSocket API、认证鉴权 |
| [05-storage-sync](docs/05-storage-sync.md) | 附件存储、同步协议、冲突解决 |
| [06-search-and-pdf](docs/06-search-and-pdf.md) | 检索、CJK 分词、PDF、AI |
| [07-integrations](docs/07-integrations.md) | Word/WPS 插件、浏览器扩展 |
| [08-security-and-deploy](docs/08-security-and-deploy.md) | 安全模型、打包分发 |
| [09-roadmap](docs/09-roadmap.md) | 路线图、里程碑、风险登记册 |
| [10-licensing](docs/10-licensing.md) | 开源许可证合规分析 |
| [11-agents](docs/11-agents.md) | 文献搜索 / 问答 Agent（pi-coding-agent） |
| [12-libraries-and-projects](docs/12-libraries-and-projects.md) | 智能文献库、论文项目库 |
| [13-knowledge-graph](docs/13-knowledge-graph.md) | 文献关系图谱 |
| [14-storage-layout](docs/14-storage-layout.md) | 存储布局、路径模板、Zotero 导入 |
| [15-development-philosophy](docs/15-development-philosophy.md) | **开发哲学：持续重构、测试、边界诚实** |
| [16-workspace-rules](docs/16-workspace-rules.md) | **工作区规则：无弹窗、tab 模型、踩坑记录** |
| [plugins/README](plugins/README.md) | **插件开发指南与协议规范** |

## 路线（尚未实现）

设计已完成、代码待补：**Word 加载项的前端（Office.js 任务窗格与清单）、多端同步**。

Word / WPS / LibreOffice 共用的**服务端协议 `/api/v1/integration/*` 已实现**：
会话按文档 id 建立、快照上传、全量重排（编号型样式插入一条会改变其后全部编号）、
只回传文本发生变化的域、样式切换触发刷新。见 `docs/07-integrations.md` 第 3.3 节。

引文关系已实现，关系图谱中有两类边：**文献耦合**（引用了相同参考文献）与**共被引**（被同一批论文一起引用）。
详见 `docs/09-roadmap.md`。

已实现并可用：条目与收藏夹、智能收藏夹、混合检索（关键词 / 模糊 / 语义 / 标签）、
查重与合并（按标识符与题名两把尺子，合并后败者进回收站而非销毁）、
导出与导入 BibTeX / RIS / CSL-JSON（导入宽容：坏记录逐条报告，其余照常）、
标注汇成笔记（按页码、原文引用与批注分开）、阅读进度记忆（存于 settings，不扰动库版本号）、备份与文件完整性检查、
标签与分面、插件系统（含徽章贡献点）、DOI/arXiv/ISBN 抓取、附件下载与 PDF 阅读标注、
文献问答与摘要 Agent、Zotero 导入（含附件、笔记、PDF 标注）、
引文与参考文献渲染（APA / MLA / Chicago / IEEE / GB-T 7714）、
关系图谱（标签 / 作者 / 收藏夹 / 语义相似 / 引用关系，含库外节点）、
必读缺口（被你多篇论文引用、你却没有的文献，一键入库）、
下载队列（可重试、可管理、重启不丢）、文件浏览器与标准化重命名、
浏览器保存（兼容 Zotero Connector 协议，用 `--connector-port 23119` 开启）。

> 浏览器保存复用已装好的 Zotero Connector 扩展：它带着二十年维护的数百个站点
> 翻译器，重写它不是好的时间投入。默认**不开启**——23119 是 Zotero 的端口，
> 悄悄占掉它会弄坏用户正要迁移的那个程序。

## 许可证

AGPL-3.0-or-later，理由见 [docs/10-licensing.md](docs/10-licensing.md)。
