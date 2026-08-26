# 13 · 文献关系图谱

## 1. 目标

把"一堆条目"变成"一张有结构的知识网络"，回答四类问题：

| 问题 | 视图 |
| --- | --- |
| 这个领域是怎么演化过来的？ | **引文时间轴**（按年份分层的 DAG） |
| 这篇论文的知识谱系？谁引它、它引谁？ | **自我中心网络**（ego network，1–2 跳） |
| 我的库分成哪几个主题簇？哪个簇我读得少？ | **主题聚类地图**（社区发现 / embedding 降维） |
| 谁是这个方向的核心人物 / 核心论文？ | **中心性排行**（PageRank / 中介中心性） + 合作网络 |

## 2. 图模型

### 2.1 节点

| 类型 | 说明 |
| --- | --- |
| `item` | 库内文献（有 `itemKey`） |
| `external` | 库外文献（只有 DOI/OpenAlexID，来自引文关系；用虚线/灰色渲染，可一键入库 ⭐） |
| `author` | 作者（带 ORCID 消歧） |
| `venue` | 期刊/会议 |
| `tag` / `concept` | 标签与主题概念（OpenAlex concepts / 自建主题模型） |
| `note` | 笔记（笔记里 `@` 引用的条目形成边） |

> **库外节点非常重要**：它让图谱能显示"你缺了哪些关键文献"—— 一个被库内 10 篇论文共同引用、但你没有的节点，几乎必然是必读文献。点击即可通过 Discovery Agent 入库。

### 2.2 边

| 边类型 | 方向 | 权重 | 来源 |
| --- | --- | --- | --- |
| `cites` | 有向 | 1 | Crossref / OpenAlex / S2 / PDF 参考文献解析 |
| `cocitation` | 无向 | 共同引用它们的文献数 | 计算得出 |
| `coupling`（文献耦合） | 无向 | 共同参考文献数 | 计算得出 |
| `similar` | 无向 | cosine 相似度 | embedding kNN |
| `authored_by` | item→author | 1 | 元数据 |
| `coauthor` | 无向 | 合作次数 | 计算得出 |
| `published_in` | item→venue | 1 | 元数据 |
| `tagged` | item→tag | 1 | 标签 |
| `related` | 无向 | 1 | 用户手动关联（`relations` 表） |
| `mentions` | note→item | 1 | 笔记链接 |

### 2.3 存储

```sql
CREATE TABLE graph_nodes (
  id          INTEGER PRIMARY KEY,
  library_id  INTEGER NOT NULL,
  kind        TEXT NOT NULL,          -- item|external|author|venue|concept|note
  item_id     INTEGER REFERENCES items(id) ON DELETE CASCADE,  -- kind=item 时
  ext_id      TEXT,                   -- doi:10.x / openalex:W123 / orcid:0000-…
  label       TEXT NOT NULL,
  year        INTEGER,
  meta        TEXT,                   -- JSON：作者、期刊、被引数等展示用字段
  UNIQUE (library_id, kind, ext_id)
);
CREATE INDEX idx_gn_item ON graph_nodes(item_id);

CREATE TABLE graph_edges (
  src        INTEGER NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
  dst        INTEGER NOT NULL REFERENCES graph_nodes(id) ON DELETE CASCADE,
  kind       TEXT NOT NULL,
  weight     REAL NOT NULL DEFAULT 1,
  source     TEXT NOT NULL,           -- crossref|openalex|s2|pdf-parse|user|computed
  confidence REAL NOT NULL DEFAULT 1, -- PDF 解析出的边置信度低
  PRIMARY KEY (src, dst, kind)
) WITHOUT ROWID;
CREATE INDEX idx_ge_dst ON graph_edges(dst, kind);

CREATE TABLE graph_metrics (          -- 定期批量计算，避免实时算
  node_id    INTEGER PRIMARY KEY REFERENCES graph_nodes(id) ON DELETE CASCADE,
  degree_in  INTEGER, degree_out INTEGER,
  pagerank   REAL,
  betweenness REAL,
  community  INTEGER,                 -- Louvain 社区号
  x REAL, y REAL,                     -- 服务端预计算的布局坐标
  updated_at INTEGER
);
```

**不引入图数据库**。规模估算：10 万条目 × 平均 30 条参考文献 ≈ 300 万边，SQLite 存下来约 200MB，全图载入内存用 `petgraph` 计算耗时秒级 —— 完全够用，且保持"单文件、零运维"的产品承诺。

## 3. 数据来源与构建

```mermaid
graph LR
  A["条目元数据"] --> G
  B["Crossref / OpenAlex / S2<br/>references + citations"] --> G
  C["PDF 参考文献解析<br/>(GROBID/AnyStyle 风格)"] --> G
  D["Embedding kNN"] --> G
  E["用户手动关联 / 笔记链接"] --> G
  G["graph_nodes + graph_edges"] --> M["批量计算<br/>PageRank / Louvain / 布局"]
  M --> V["前端可视化"]
```

- **拉取策略**：入库时后台异步拉取该文献的 references/citations（限速、缓存、可离线跳过）。库外节点只存轻量元数据。
- **实体消歧**：DOI 优先；无 DOI 时用 `归一化标题 + 首作者 + 年份` 指纹；作者用 ORCID > (姓名 + 机构 + 领域) 聚类。中文作者重名严重，额外用共同作者网络辅助消歧。
- **增量**：新条目入库 → 只计算与已有图的连接，不重建全图；`pagerank`/`community` 每 N 次变更或每日重算一次。
- **离线可用**：没有网络时，图谱退化为「基于本地元数据 + PDF 解析 + embedding」的版本，仍可用。

## 4. 计算

| 算法 | 用途 | 实现 |
| --- | --- | --- |
| PageRank | 找核心文献 | `petgraph` + 自研幂迭代 |
| Louvain / Leiden | 主题社区发现 | Rust 实现（社区数 → 自动命名：取簇内高频概念 + LLM 起名） |
| 中介中心性（近似） | 找"桥梁"论文（连接两个领域） | Brandes 采样近似 |
| 最短路径 | "从 A 到 B 的引文路径"，讲清楚思想传承 | BFS/Dijkstra |
| ForceAtlas2 / OpenOrd | 布局 | 节点 > 5000 时**服务端预计算**存 `graph_metrics.x/y`；小图前端 Worker 实时布局 |
| UMAP（可选） | embedding 二维投影 → "主题地图" | `fastembed` + 轻量 UMAP 实现，或 t-SNE 替代 |

## 5. 前端可视化

**选型：`graphology`（图数据结构）+ `sigma.js`（WebGL 渲染）+ `graphology-layout-forceatlas2`（Web Worker）**
理由：sigma.js 在 WebGL 下可流畅渲染万级节点，且与 graphology 生态（布局、度量）无缝；D3 力导向在 2000 节点以上就卡。

### 5.1 交互设计

```
┌────────────────────────────── 图谱视图 ─────────────────────────────┐
│ [视图: 引文网络▾] [范围: 项目《综述》▾] [布局: 力导向▾] [🔍搜索]      │
│ ┌─ 侧栏 ────┐ ┌──────────── 画布 ───────────────┐ ┌─ 详情 ──────┐ │
│ │边类型开关  │ │   ●━━━▶●                        │ │ 选中节点     │ │
│ │ ☑ 引用     │ │  ╱      ╲                       │ │ 标题/作者    │ │
│ │ ☑ 共被引   │ │ ●   ◌ ← 库外节点(虚线)          │ │ PageRank #7 │ │
│ │ ☐ 语义相似 │ │  ╲      ╱                       │ │ [打开]      │ │
│ │节点着色    │ │   ●━━━▶●                        │ │ [加入库]⭐  │ │
│ │ ○社区 ●年份│ │                                 │ │ [以此为中心]│ │
│ │ ○阅读状态  │ │  时间轴 ◀━━━━━━━━━━━▶ 2015—2026 │ │ [问 Agent] │ │
│ └───────────┘ └─────────────────────────────────┘ └────────────┘ │
└────────────────────────────────────────────────────────────────────┘
```

- **渐进披露**：默认只画 `cites` 边 + 库内节点；用户按需打开共被引、语义相似、库外节点。一次性全画必然变成"毛球图"（hairball），是所有图谱功能失败的主因。
- **节点大小** = PageRank 或被引数；**颜色** = 社区 / 年份 / 阅读状态 / 项目章节；**虚线圈** = 库外文献。
- **聚焦模式**：双击节点 → 只保留其 1–2 跳邻域，其余淡出。
- **时间轴刷选**：拖动年份区间，图动态过滤，看领域演化。
- **框选 → 批量操作**：选中一个簇 → 「全部加入项目」/「让 Agent 解释这个簇」/「导出 bib」。
- **性能**：节点 > 3000 时自动切"聚合模式"（按社区折叠成超级节点，展开才细化）；坐标走服务端预计算；渲染走 WebGL；数据用二进制/紧凑 JSON 传输。

### 5.2 与 Agent 联动

| 图上操作 | Agent 行为 |
| --- | --- |
| 选中一个社区 → "解释" | Library Agent 读该簇文献，输出主题概括 + 代表作 + 演化脉络 |
| 选中两个节点 → "它们什么关系" | 找引文路径 + 精读 + 说明思想传承或分歧 |
| "找出我的盲区" | 统计高入度的库外节点 → Discovery Agent 补齐 → 进收件箱 |
| 选中节点 → "找最新跟进" | Discovery Agent 拉该文的 citations 并按时间/质量排序 |

## 6. API

```
GET  /api/v1/libraries/{lib}/graph
     ?scope=collection:QWERTY12|project:3|smart:K1|items:A,B,C|all
     &edges=cites,cocitation           # 边类型开关
     &includeExternal=true&minDegree=2 # 库外节点过滤
     &layout=precomputed|none
     &maxNodes=3000
     → { nodes:[{id,kind,label,year,x,y,pagerank,community,itemKey?}],
         edges:[{s,d,k,w}], stats:{...}, truncated:false }

GET  /api/v1/items/{key}/graph/ego?depth=2&direction=both
GET  /api/v1/graph/path?from=&to=
GET  /api/v1/libraries/{lib}/graph/communities        → [{id, label, size, topItems[], topConcepts[]}]
GET  /api/v1/libraries/{lib}/graph/gaps               → 高入度的库外文献（"你缺的关键文献"）
POST /api/v1/libraries/{lib}/graph/rebuild            → 异步重算（taskId）
POST /api/v1/graph/nodes/{id}/import                  → 库外节点一键入库
GET  /api/v1/libraries/{lib}/graph/export?format=graphml|gexf|json|csv
```
支持导出 GraphML/GEXF，方便用户拿去 Gephi / VOSviewer 做深度分析 —— **不锁死用户数据**。
