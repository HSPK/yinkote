# 04 · API 设计

## 1. 总则

- Base：`http://127.0.0.1:23130/api/v1`（本地 HTTPS：`https://127.0.0.1:23131/api/v1`）
- 传输：JSON（`application/json; charset=utf-8`），文件用二进制流 / `multipart`
- 契约：Rust 侧 `utoipa` 标注 → 生成 `openapi.json` → `openapi-typescript-codegen` 产出 `packages/api-client`
- 所有客户端（Web / Word / 扩展 / CLI）**只允许**使用这套公开 API，无内部后门接口

### 通用响应头

| Header | 含义 |
| --- | --- |
| `Last-Modified-Version` | 当前库版本，用于增量同步 |
| `Total-Results` | 分页总数 |
| `Link` | RFC 5988 分页链接（`rel="next"`） |
| `X-Yinkote-Version` | 服务端版本 |

### 错误格式（RFC 9457 Problem Details）

```json
{
  "type": "https://yinkote.dev/errors/version-conflict",
  "title": "Item has been modified",
  "status": 412,
  "detail": "Item A1B2C3D4 is at version 130, you sent 127",
  "instance": "/api/v1/libraries/1/items/A1B2C3D4",
  "extra": { "currentVersion": 130 }
}
```

常用状态码：`400` 参数错 · `401` 未认证 · `403` scope 不足 · `404` · `409` 冲突（如 key 重复） · `412` 版本冲突 · `413` 文件过大 · `422` schema 校验失败 · `429` 限流 · `507` 磁盘不足。

## 2. 认证与鉴权

### 三种凭证

| 凭证 | 使用者 | 说明 |
| --- | --- | --- |
| **Session Cookie** | Web 工作台 | `POST /auth/login` 后下发 `HttpOnly; SameSite=Lax; Secure(https 时)`；配合 CSRF token |
| **API Key（Bearer）** | Word 插件、浏览器扩展、CLI | `Authorization: Bearer yk_live_xxx`；带 scope，可命名、可吊销、可设过期 |
| **配对码（Pairing Code）** | 插件首次授权 | 插件请求配对 → 服务端在托盘/Web 弹出 6 位码 → 用户确认 → 换取 API Key（OAuth Device Flow 简化版） |

```
POST /api/v1/auth/pair/request  { "client": "Chrome Extension", "scopes": [...] }
   → { "pairingId": "...", "userCode": "K3F-92X", "expiresIn": 300, "interval": 2 }
POST /api/v1/auth/pair/poll     { "pairingId": "..." }
   → 202 pending  |  200 { "apiKey": "yk_live_...", "scopes": [...] }
```
这样插件**永远不需要用户手动复制粘贴 token**，也不需要在插件里输入密码。

### Scope 列表

`items:read` `items:write` `files:read` `files:write` `annotations:write` `citation:render` `translate:run` `search` `graph:read` `staging:write` `agents:run` `admin`

**`agentd` 拿到的 Key 只含**：`items:read` `files:read` `search` `graph:read` `staging:write` `citation:render` `translate:run` —— **没有 `items:write`**，从协议层杜绝 Agent 擅自改库。

### 单用户本地模式

默认部署是单用户。首次访问 `/setup` 创建管理员。可在 config 中开启 `local_trust = true`：来自 `127.0.0.1` 且 `Origin` 为空/本机的请求免登录（方便 CLI），但**默认关闭**，且远程绑定时强制关闭。

## 3. 资源端点

### 3.1 库 / 收藏夹

```
GET    /libraries
GET    /libraries/{lib}/collections?since=&format=tree
POST   /libraries/{lib}/collections            批量创建
PATCH  /libraries/{lib}/collections/{key}
DELETE /libraries/{lib}/collections/{key}?recursive=true
GET    /libraries/{lib}/collections/{key}/items
```

### 3.2 条目（核心）

```
GET    /libraries/{lib}/items
       ?q=&qmode=titleCreatorYear|everything
       &itemType=journalArticle||book        # || 表示 or，- 前缀表示非
       &tag=machine%20learning&tag=-obsolete
       &collection=QWERTY12&recursive=true
       &since=128&includeTrashed=false
       &sort=dateModified&direction=desc
       &start=0&limit=100
       &include=data,csljson,bib,citation&style=gb-t-7714-2015&locale=zh-CN
GET    /libraries/{lib}/items/{key}
GET    /libraries/{lib}/items/{key}/children     # 附件 + 笔记
POST   /libraries/{lib}/items                    # 批量写，body 为数组（≤50）
PATCH  /libraries/{lib}/items/{key}              # 局部更新 + If-Unmodified-Since-Version
DELETE /libraries/{lib}/items?itemKey=A,B,C      # 进回收站
POST   /libraries/{lib}/items/{key}/restore
DELETE /libraries/{lib}/trash                    # 清空
POST   /libraries/{lib}/items/merge              { master, others[] }
POST   /libraries/{lib}/items/duplicates/check   { candidates[] } → 命中列表
```

**批量写返回 Zotero 风格的部分成功**（一个失败不阻塞其余）：
```json
{ "success": { "0": "A1B2C3D4" },
  "unchanged": { "1": "E5F6G7H8" },
  "failed": { "2": { "code": 422, "message": "invalid itemType 'foo'" } } }
```

### 3.3 附件与文件

```
POST   /libraries/{lib}/items                       先创建 attachment 条目（拿 key）
POST   /libraries/{lib}/items/{key}/file            上传（支持 Content-Range 分片续传）
       Headers: If-None-Match: <blake3>  → 已存在则 204，实现秒传/去重
GET    /libraries/{lib}/items/{key}/file            下载（支持 Range，供 pdf.js 按需取页）
GET    /libraries/{lib}/items/{key}/file/view       内联查看（Content-Disposition: inline）
GET    /libraries/{lib}/items/{key}/thumbnail?page=1&w=240
DELETE /libraries/{lib}/items/{key}/file
POST   /libraries/{lib}/items/{key}/reveal          在系统文件管理器中显示
```

> **`reveal` 不需要 Tauri 壳**：server 本来就跑在用户自己的机器上，直接起
> 文件管理器即可。两条规则撑起这个端点的全部安全性：
> **① 客户端只给 key，不给路径**——路径由服务端按自己的存储布局解析出来；
> 一个 `path` 参数会让它变成"远程打开磁盘上任意文件"的原语，而且浏览器里
> 任何一个页面都够得着。**② 全程不经 shell**——`command()` 返回
> 程序名与 argv 数组直接交给 `Command`，文件名里的 `;` 或 `$(…)` 没有任何
> 地方会被解释。
>
> 各平台都要求"选中"而非"打开所在目录"：macOS `open -R`、
> Windows `explorer /select,<path>`（逗号是标志的一部分，路径必须粘在同一个
> 参数上，拆成两个会静默打开"文档"目录）、Linux 走 D-Bus 的
> `org.freedesktop.FileManager1.ShowItems`——对文件用 `xdg-open` 会把它*打开*，
> 那是另一个功能。无桌面会话时**如实报错**而不是回 ok：两种情况下屏幕上都
> 什么都不会发生，用户无从分辨。见 `crates/yk-server/src/routes/reveal.rs`。

### 3.4 标注与阅读器

```
GET    /libraries/{lib}/items/{attKey}/annotations?since=
POST   /libraries/{lib}/annotations                 批量 upsert
DELETE /libraries/{lib}/annotations?key=...
GET    /libraries/{lib}/items/{attKey}/reader-state  → { lastPage, zoom, scrollMode, sidebar }
PUT    /libraries/{lib}/items/{attKey}/reader-state
POST   /libraries/{lib}/items/{key}/notes/from-annotations  { annotationKeys[], template }
```

### 3.5 标签 / 检索

```
GET    /libraries/{lib}/tags?q=&limit=            (含 count，支持标签云)
PATCH  /libraries/{lib}/tags/{name}               改名/改色（全局替换）
DELETE /libraries/{lib}/tags/{name}
GET    /libraries/{lib}/searches
POST   /libraries/{lib}/searches
POST   /libraries/{lib}/search/fulltext           { q, filters, highlight: true }
```

`saved_search` 条件树：
```json
{ "op": "all", "conditions": [
    { "field": "itemType", "op": "is", "value": "journalArticle" },
    { "field": "tag", "op": "contains", "value": "LLM" },
    { "op": "any", "conditions": [
        { "field": "date", "op": "isInTheLast", "value": "3 years" },
        { "field": "fulltext", "op": "contains", "value": "扩散模型" }] } ] }
```

### 3.6 元数据识别与抓取

```
POST /translate/identifier   { "text": "10.1038/nature12373  arXiv:1706.03762  9787111213826" }
     → 解析出多个标识符并返回候选条目（Crossref / OpenAlex / PubMed / arXiv / OpenLibrary / CNKI）
POST /translate/web          { "url", "html?", "cookie?" }  → items[]（服务端 translator）
POST /translate/import       multipart: .bib/.ris/.enw/.rdf/.json/.nbib  → items[]
POST /translate/pdf          multipart: PDF  → 从 DOI/首页文本/参考文献解析元数据
POST /translate/export       { itemKeys[], format: "bibtex|ris|csljson|zotero-rdf" } → 文件流
POST /translate/citation-parse  { text: "参考文献纯文本列表" } → 结构化条目（AnyStyle/GROBID 风格）
```

### 3.7 引文渲染

```
GET  /styles                          已安装样式（id, title, 是否 note 型, 依赖）
POST /styles/install                  { url | file }（从 CSL 仓库或本地）
GET  /locales
POST /citation/preview                { itemKeys[], styleId, locale, mode: "citation"|"bibliography" }
POST /citation/bibliography           { itemKeys[], styleId, locale } → { entries[], html, rtf }
POST /citation/quick-copy             { itemKeys[], format: "html|text|markdown|rtf" }
```

### 3.8 Word 集成会话协议

见 [07-integrations](07-integrations.md)，端点前缀 `/integration/*`。

### 3.9 浏览器扩展（含 Zotero Connector 兼容层）

```
GET  /connector/ping                       → { version, prefs, selected: {...} }
POST /connector/getSelectedCollection      → 当前工作台选中的收藏夹
POST /connector/saveItems                  { items: [...], uri, sessionID }
POST /connector/saveSnapshot               { url, html, pdf? }
POST /connector/saveSingleFile             （完整页面快照）
POST /connector/sessions/{id}/progress     上传进度轮询
POST /connector/updateSession              { sessionID, target, tags }  改收藏夹/加标签
```
> 兼容 Zotero Connector 的报文形状可让**官方 Zotero 扩展直接把文献存进 Yinkote**，是极佳的冷启动策略。见 10-licensing 的合规讨论。

### 3.10 扩展模块

以下端点族在各自文档中定义：

| 端点前缀 | 模块 | 文档 |
| --- | --- | --- |
| `/agents/*` | Agent 会话、暂存区、待确认区、监测任务 | [11-agents](11-agents.md) |
| `/projects/*`、`/smart-libraries/*`、`/inbox` | 论文项目库、智能文献库、收件箱 | [12](12-libraries-and-projects.md) |
| `/graph/*`、`/items/{key}/graph/*` | 关系图谱 | [13](13-knowledge-graph.md) |
| `/maintenance/reorganize/*` | 存储路径重组织 | [14](14-storage-layout.md) |

### 3.11 系统

```
GET  /ping                 → { apiVersion, serverVersion, minClientVersion, libraries[] }  免认证
GET  /tasks?state=running
POST /tasks/{id}/cancel
GET  /settings  /  PUT /settings
POST /maintenance/reindex      重建全文索引
POST /maintenance/backup       立即备份
GET  /maintenance/integrity    检查缺失附件 / 孤儿文件
POST /import/zotero            { dataDir }  从 Zotero 数据目录迁移
```

## 4. WebSocket

```
WS /api/v1/events?token=...
← { "type":"hello", "libraries": {"1": 128} }
← { "type":"library.version", "libraryId":1, "version":131 }
← { "type":"item.changed", "libraryId":1, "keys":["A1B2C3D4"], "version":131 }
← { "type":"task.progress", "taskId":42, "kind":"pdf_extract", "pct":63 }
← { "type":"connector.saving", "sessionId":"...", "title":"Attention Is All..." }
→ { "type":"subscribe", "libraries":[1] }
```
前端策略：收到 `item.changed` 后用 TanStack Query 精准失效对应 key，避免整表刷新；断线用指数退避重连，重连后以 `since=<本地版本>` 补齐差量。

## 5. 性能与稳健

| 措施 | 说明 |
| --- | --- |
| 分页 | 默认 `limit=50`，上限 100；深翻页用 `(sortKey, id)` 游标而非 OFFSET |
| 条件请求 | `ETag`/`If-None-Match`、`Last-Modified-Version`/`If-Modified-Since-Version` |
| 压缩 | `tower-http` gzip/br |
| 限流 | 每 API Key 令牌桶；抓取类端点单独限速，保护上游站点 |
| 幂等 | 写请求可带 `Idempotency-Key`，重放返回同一结果 |
| 大文件 | 上传分片 + 断点续传；下载支持 Range |
| 超时 | translate/AI 类端点异步化：返回 `202 + taskId`，经 WS 推进度 |

## 6. API 示例

```bash
# 1. 用 DOI 抓一篇文献进"待读"收藏夹
curl -s -X POST http://127.0.0.1:23130/api/v1/translate/identifier \
  -H "Authorization: Bearer $YK_KEY" -H 'Content-Type: application/json' \
  -d '{"text":"10.1038/nature12373"}' | jq '.items[0]' > item.json

curl -s -X POST http://127.0.0.1:23130/api/v1/libraries/1/items \
  -H "Authorization: Bearer $YK_KEY" -H 'Content-Type: application/json' \
  -d "[$(cat item.json)]"

# 2. 生成 GB/T 7714 参考文献
curl -s -X POST http://127.0.0.1:23130/api/v1/citation/bibliography \
  -H "Authorization: Bearer $YK_KEY" -H 'Content-Type: application/json' \
  -d '{"itemKeys":["A1B2C3D4"],"styleId":"gb-t-7714-2015","locale":"zh-CN"}'
```
