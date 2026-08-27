# 05 · 存储与同步

> 附件路径模板、sidecar 元数据规范、Zotero 迁移细节见 [14-storage-layout](14-storage-layout.md)；本文聚焦**同步与备份**。

## 1. 附件存储

### 存储模式

| 模式 | 说明 | 场景 |
| --- | --- | --- |
| `imported_file` | 文件复制进 `library/`（路径由模板决定）由 Yinkote 托管 | 默认 |
| `imported_url` | 同上，但记录来源 URL（网页快照、下载的 PDF） | 抓取 |
| `linked_file` | 只存路径，文件留在用户目录（如 Zotfile 式的 `~/Papers/`） | 已有文献目录 / 与 NAS 共存 |
| `linked_url` | 仅链接，不存文件 | 在线资源 |

要点：
- **内容寻址去重**：入库先算 `blake3`，若库内已有同哈希文件则做硬链接/引用计数，节省空间（大量重复 PDF 场景很常见）。
- **linked_file 目录监听**：用 `notify` 监听基准目录，文件被移动/重命名时自动修复路径（Zotero 的老痛点）。
- **文件名模板**：`{{creator}}_{{year}}_{{title|truncate 60}}.pdf`，可配置，重命名时同步更新 DB。
- **完整性检查**：`GET /maintenance/integrity` 扫描"DB 有记录但文件缺失"与"目录有文件但无记录"的孤儿。
  **已实现**，只报告、不修复：数据库出错时，因为"数据库说不出这个文件的来历"就删掉它，
  恰恰是最坏的反应。整库一次 `spawn_blocking` 完成目录遍历与 `stat`（见 `docs/16` §3.60）。

### 网页快照

优先用扩展侧的 **SingleFile** 方案（把 CSS/图片内联成单个 `.html`），服务端只负责存储；服务端抓取作为降级（无 JS 渲染）。

## 2. 备份

- 每日（可配置）生成 `backups/yinkote-YYYYMMDD.db`，保留最近 N 份（默认 7）+ 每月 1 份。
  **已实现**：`POST /maintenance/backup`、`GET /maintenance/backups`。
  用 `VACUUM INTO` 而非 Online Backup API：同样取一致快照，但写出的是**压缩过**的副本
  （删过大批导入的库可以小到三分之一），且目标已存在时直接拒绝——不会留下半份文件
  被误当成备份。保留规则是纯函数 `maintenance::backups::prune`，返回"该删哪些"
  而不是"该留哪些"（见 `docs/16` §3.70）。
- 迁移前强制快照。
- `POST /maintenance/export-all` 导出 `.yinkote`（zip：`db.sqlite` + `library/` + `metadata/` + `manifest.json`），可整包迁移到另一台机器。

## 3. 同步：分层策略

Yinkote 的默认形态是**单服务端权威**，不需要同步。同步只在两种场景出现：

- **场景 S1（远程访问）**：一台常开机器跑 Server，其它设备用浏览器访问 → **无需同步**，最推荐。
- **场景 S2（多端各自本地实例，需合并）**：笔记本 + 台式机各跑一个 Server → 需要同步。

### 3.1 同步后端（可插拔 trait `SyncBackend`）

| 后端 | 元数据 | 附件 | 说明 |
| --- | --- | --- | --- |
| **Yinkote Node**（自建，同一份 server 二进制加 `--mode=hub`） | ✅ | ✅ | 体验最佳，支持增量与冲突反馈 |
| **WebDAV**（坚果云/NextCloud） | ✅（打包 changeset） | ✅（zip per attachment） | 兼容 Zotero 用户既有习惯 |
| **S3 兼容**（MinIO/R2/OSS） | ✅ | ✅ | 大库、便宜 |
| **纯文件夹**（配合 Syncthing/iCloud） | ⚠️ | ✅ | 仅同步 `library/`，DB 不放进去（易损坏），元数据仍走上面三种 |

> **重要红线**：绝不建议把 `yinkote.db` 直接丢进 Dropbox/iCloud/Syncthing —— SQLite + 云盘 = 数据损坏。UI 会检测数据目录是否位于常见云盘路径并告警。

### 3.2 元数据同步协议（基于版本号）

```
① 拉取远端库版本      GET  /hub/libraries/{lib}/version           → remoteVersion
② 若 remote > localLastSync：
   拉取增量           GET  /hub/libraries/{lib}/changes?since=L    → { objects[], deletions[], version }
   本地应用（三方合并）
③ 上传本地改动        POST /hub/libraries/{lib}/changes
   Header: If-Unmodified-Since-Version: remoteVersion
   → 200 全部成功 / 412 期间有人改过 → 回到 ②
④ 附件同步（见 3.4）
⑤ 记录 localLastSync = 新的库版本
```

要素：
- **三方合并**：`base`（上次同步的快照，存 `sync_base` 表）、`local`、`remote`。字段级合并 —— 不同字段各改各的可自动合并；同字段冲突才提示。
- **冲突策略**：
  - 元数据字段冲突 → 写入 `conflicts` 表，UI 提供并排对比与"取本地/取远端/手动"三选一；默认**保留双方**（远端值写入 `extra` 备注），不静默丢数据。
  - 笔记（富文本）冲突 → 保留两份，标题加 `(冲突副本 2026-08-26)`；V1.x 引入 Yjs CRDT 后可自动合并。
  - 标注 → 以 `annotationKey` 为粒度，天然低冲突；同一标注的 comment 冲突走上面的字段策略。
  - 删除 vs 修改 → 保守：恢复条目并标记待确认。

### 3.3 端到端加密（可选，V1.x）

- 用户口令 → Argon2id → 主密钥（不上传）。
- 元数据 JSON 与附件用 XChaCha20-Poly1305 加密后上传；文件名与目录用确定性加密派生，保证同步后端只见密文。
- 代价：服务端搜索能力丧失（Hub 只做 blob 存储），可接受，因为搜索本就在本地做。

### 3.4 附件同步

- 元数据同步完成后，按 `hash` 比对差异清单。
- 上传：`PUT /hub/files/{hash}`（内容寻址 → 天然去重、天然幂等）；`HEAD` 先探测存在性实现秒传。
- 下载：可选 **按需下载**（`storage_state = remote_only`，点击打开时才拉），大库友好。
- 传输并发 4，断点续传，失败指数退避。

## 4. 从 Zotero 迁移

`POST /import/zotero { "dataDir": "/Users/x/Zotero" }`

1. 只读打开 `zotero.sqlite`（复制到临时目录避免锁）。
2. 映射：`items/itemData/itemDataValues/fields` → `items.data`；`creators` → `item_creators`；`collections`、`itemTags`、`itemNotes`、`itemAttachments`、`annotations`（Zotero 6+）、`relations` 全量搬。
3. 附件：从 Zotero 的 `storage/<key>/` 复制到 Yinkote 的 `library/`，导入时即按路径模板重命名（可关闭）；`linked_file` 保留原路径并做可达性检查。
4. 保留 Zotero 的 `key`，并在 `relations` 写 `owl:sameAs zotero://…`，将来可双向对照。
5. 输出迁移报告：条目数、跳过项、字段丢失清单，供用户核对。

> 完整的字段映射表、Better BibTeX citekey 保留、dry-run 与回滚机制见 [14-storage-layout §6](14-storage-layout.md)。

同时支持从 **EndNote (XML)**、**Mendeley (sqlite/BibTeX)**、**JabRef (.bib)**、**NoteExpress (导出 RIS)** 导入。
