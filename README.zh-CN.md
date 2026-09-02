# 引可特 Yinkote

**自己运行、在浏览器里使用的本地优先文献管理工具。**

[English](README.md) · [文档](docs/) · AGPL-3.0-or-later

引可特只有一个可执行文件。在自己的电脑上运行它，打开浏览器，文献库就在那里。
不上传任何东西，不需要注册账号，断网也照常工作。

如果你希望自己的文献存放在看得见的文件夹里、用得懂的格式中、跑在自己掌控的机器上，
那么它是 Zotero 的一个可用替代品。

```
┌─ 你的电脑 ──────────────────────────────────┐
│  yinkote  ──►  SQLite + 磁盘上的 PDF        │
│     ▲                                        │
│     │  HTTP + WebSocket，监听 127.0.0.1      │
│     ├── 浏览器里的工作台                     │
│     ├── Word 加载项                          │
│     └── 浏览器扩展                           │
└──────────────────────────────────────────────┘
```

> **状态：v0.1，早期版本。** 库格式已经稳定，测试也算充分——后端 903 个测试、
> 工作台 627 个、对着运行中的服务器还有 281 项冒烟检查——但它还没有在很多人的
> 很多台机器上跑过。请像对待任何存放多年阅读记录的东西一样，做好备份。

---

## 安装

下载对应平台的可执行文件，加上执行权限，运行。

```bash
# macOS / Linux
chmod +x yinkote
./yinkote
```

```powershell
# Windows
.\yinkote.exe
```

然后打开 **<http://127.0.0.1:23130>**。

安装到此结束。没有安装程序，不需要先装运行时，也不用写配置文件。
首次运行会自己建好数据目录并开始服务。

| 平台 | 文件 |
| --- | --- |
| Linux (x86-64) | `yinkote-x86_64-unknown-linux-gnu` |
| Linux (ARM64) | `yinkote-aarch64-unknown-linux-gnu` |
| macOS（Apple 芯片） | `yinkote-aarch64-apple-darwin` |
| macOS（Intel） | `yinkote-x86_64-apple-darwin` |
| Windows (x86-64) | `yinkote-x86_64-pc-windows-msvc.exe` |

**为什么能做到一个文件。** 工作台被编译进了二进制，SQLite 是静态链接的，
唯一的动态依赖是系统的 C 运行时。20 MB 的下载就是程序的全部。

### 开机自启

```bash
yinkote service install      # 登录时自动启动
yinkote service status
yinkote service uninstall
```

按平台分别写入 systemd **用户**单元、launchd agent，或"启动"文件夹里的脚本。
不会安装成系统服务：个人的文献库不该属于 root。

### 之后再打开工作台

```bash
yinkote open
```

它从数据目录自己的锁文件里读出**已经在运行**的那个服务器的地址，然后用浏览器打开，
而不是再启动一个。

---

## 你的数据

所有东西都在一个目录里，可以整体复制、备份，或者放进同步盘。

```
<数据目录>/
├─ yinkote.db          文献库：条目、笔记、标签、收藏夹、索引
├─ storage/            附件，每个条目一个文件夹
├─ plugins/            已安装的插件
└─ config.toml         只有改过设置才会写
```

未用 `--data-dir` 指定时的默认位置：

| 平台 | 位置 |
| --- | --- |
| Linux | `$XDG_DATA_HOME/yinkote`，否则 `~/.local/share/yinkote` |
| macOS | `~/Library/Application Support/Yinkote` |
| Windows | `%APPDATA%\Yinkote` |

数据库就是普通的 SQLite，附件就是普通文件夹里的普通文件。
即使引可特明天消失了，你的文献库还在。

### 从 Zotero 迁移

在 Zotero 里导出文献库（**文件 → 导出文献库**，选 *Zotero RDF* 并包含文件，
或者直接让引可特读取 Zotero 的数据目录），然后在 **设置 → 导入** 里导入。
条目、收藏夹、标签、笔记、批注和附件都会带过来，重复项会被合并而不是翻倍。

---

## 从别的设备访问

默认只监听 `127.0.0.1`：本机之外谁也够不着，因此也不需要密码——没有别人能问。

要从手机或另一台电脑访问：

```bash
yinkote --host 0.0.0.0
```

第一次这么做时，引可特会**拒绝在没有 API key 的情况下启动**，并在报错里直接给出办法。
一旦绑定超出回环地址，那些让"本机无密码"变得安全的浏览器防护就失效了，
所以这个 key 不是可选项：

```bash
YK_API_KEY="一串足够长的随机字符" yinkote --host 0.0.0.0
```

想让它保留下来，就写进数据目录里的 `config.toml`：

```toml
api_key = "一串足够长的随机字符"
```

之后 API 请求带上 `Authorization: Bearer <key>`；工作台会问一次并记住。

`--allow-anonymous` 确实存在，而且是个坏主意：它把整个文献库——包括删除条目、
读取文件——暴露给任何能连上这个端口的人。真要远程访问，请把引可特放在
Tailscale、带 TLS 的反向代理，或 SSH 隧道后面。

### 浏览器扩展

```bash
yinkote --connector-port 23119
```

引可特会在 Zotero 连接器期望的端口上应答，于是 Zotero 浏览器扩展就会把文献
存进引可特。默认关闭：那个端口属于 Zotero，占用它会弄坏正在运行的 Zotero。

### Word / WPS

加载项由正在运行的服务器提供。**设置 → Word 加载项** 里有清单路径和对应平台的
旁加载步骤。

---

## 它能做什么

**条目与组织。** 由 schema 驱动的 17 种文献类型，而不是写死的表单；无限层级收藏夹；
本质是保存的搜索的智能收藏夹；带颜色的标签；一个在你清空之前真的会留着东西的回收站。

**真的找得到的搜索。** 四种策略融合成一个排序：关键词（BM25）、模糊（三元组 + 编辑距离，
拼错了也能找到）、语义（向量）、字段过滤。查询语法就是你猜的那样——
`tag:综述 type:journalArticle author:hinton year:2020..2024 -tag:已归档 "精确短语"`——
中文不需要任何配置就能搜。

**阅读。** PDF 阅读器带高亮、笔记和大纲，按设备分辨率渲染。任何文献都可以写 Markdown 笔记。
批注可以一键汇集成一条笔记。

**参考文献。** 来自 Crossref；预印本来自 Semantic Scholar；都没有时，直接从 PDF 页面上读。
返回结果会说明来源，因为这三者的可靠程度并不相同。
"库里引用了但没有收藏"的文献单独成页。

**把文献收进来。** 粘贴 DOI、arXiv 链接、PubMed ID、ISBN 或者一个普通网址，
引可特会判断这是什么、取回元数据、并把 PDF 排进下载队列。
把文献归入收藏夹时也会顺带获取它的文件。

**AI，如果你要的话。** 摘要、精读，以及一个能搜索、归类、打标签的全库助手——
全部对着**你自己**配置的端点。没有内置的云服务，默认不向任何地方发送任何东西。
指向本地的 Ollama 或 llama.cpp，它就一步也不出这台机器。

**插件。** 独立进程，用 JSON-RPC 通信，能力需要声明，对数据库没有特权访问。
仓库里带了三个示例，其中包括期刊指标（影响因子、JCR、中科院分区）。

**引用。** CSL 样式、对任意选择生成参考文献表、Word 里的活引用域。

---

## 性能

在本机对一个 **99,898 条目** 的库测得，release 构建，全部条目已向量化：

| 操作 | p50 | p95 |
| --- | --- | --- |
| 关键词搜索（两个词） | 12.8 ms | 14.5 ms |
| 关键词搜索（一个词） | 26.1 ms | 46.0 ms |
| 中文关键词搜索 | 33.0 ms | 37.1 ms |
| 模糊搜索（拼错） | 5.7 ms | 6.5 ms |
| 语义搜索 | 6.7 ms | 8.3 ms |
| 混合搜索（四路融合） | 13.9 ms | 16.0 ms |
| 混合搜索 + 取出要显示的行 | 34.8 ms | 41.0 ms |
| 打开一个收藏夹 | 3.0 ms | 3.4 ms |
| 文件浏览器一页 | 6.7 ms | 7.6 ms |
| 新建一个条目 | 3.3 ms | 3.9 ms |

`node scripts/bench.mjs` 可以复现这些数字，库里没有语料时它会自己灌。
**它会往你指定的那个库里灌十万条数据**，所以请给它一个临时数据目录。

---

## 参数

```
yinkote [OPTIONS]
yinkote open
yinkote service install|uninstall|status
```

| 参数 | 含义 |
| --- | --- |
| `-p, --port <PORT>` | 监听端口（默认 `23130`） |
| `--host <HOST>` | 绑定地址（默认 `127.0.0.1`） |
| `--data-dir <DIR>` | 文献库所在目录 |
| `--web-dir <DIR>` | 从磁盘提供工作台，而不用内置的那份 |
| `--plugin-dir <DIR>` | 额外的插件目录，可重复 |
| `--connector-port <PORT>` | 同时应答 Zotero 浏览器扩展 |
| `--allow-anonymous` | 公开地址且不要 key。请先读上面的警告。 |

环境变量：`YK_DATA_DIR` `YK_HOST` `YK_PORT` `YK_WEB_DIR` `YK_API_KEY` `YK_LOG`，
以及嵌入提供方的 `YK_EMBED_*` 和助手的 `YK_AGENT_*`。

---

## 从源码构建

需要 Rust 1.85+ 和 Node 20+。先构建前端，因为二进制会把它嵌进去。

```bash
(cd web && npm install && npm run build)
cargo build --release -p yk-server
./target/release/yinkote
```

没有 `web/dist` 也能编译、也能运行：它会提供一个页面说明工作台没有构建，
这比一片空白是更好的失败方式。

### 参与开发

```bash
cargo run -p yk-server -- --data-dir ./.dev-data     # 后端
(cd web && npm run dev)                              # 前端 5273 端口，自动代理 /api

cargo test --workspace                               # 903 个测试
cargo clippy --workspace --all-targets -- -D warnings
(cd web && npm test)                                 # 627 个测试
bash scripts/smoke.sh                                # 对运行中的服务器做 281 项检查
node scripts/bench.mjs                               # 上面那些数字
```

动手改之前值得先读 `docs/15-development-philosophy.md` 和
`docs/16-workspace-rules.md`。后者是这个项目已经犯过的错误、以及各自的代价的
长长一份清单，也是整个仓库里最有用的东西。

---

## 代码是怎么组织的

```
crates/
├─ yk-core      领域模型、端口（trait）、错误、事件、条目 schema
├─ yk-store     SQLite：仓储、迁移、FTS/三元组/向量队列的维护
├─ yk-search    混合检索：BM25 + 模糊 + 向量，融合排序
├─ yk-pdf       文本抽取、参考文献解析
├─ yk-scrape    标识符解析、元数据源、外部检索
├─ yk-cite      CSL 引用与参考文献表渲染
├─ yk-ai        嵌入与对话提供方的抽象
├─ yk-agent     助手：工具、回合、技能
├─ yk-import    Zotero 与文献表导入
├─ yk-plugin    插件运行时：发现、JSON-RPC、钩子、生命周期
└─ yk-server    HTTP/WebSocket、后台任务、内嵌的工作台
web/            React + TypeScript 工作台
plugins/        示例插件
```

依赖严格向内：`yk-server → yk-{store,search,plugin} → yk-core`。
所有跨层协作都经由 `yk-core::ports` 里的 trait，因此检索引擎、插件运行时、
嵌入提供方都可以整体替换而不触碰调用方。

几个撑得住的设计决定：

- **派生数据在写入的同一个事务里维护。** 全文索引、三元组索引和嵌入队列不可能与
  条目漂移，因为不存在只更新其中之一的路径。
- **写事务用 `BEGIN IMMEDIATE`。** 延迟事务在并发下无法升级锁，SQLite 会立刻返回
  `SQLITE_BUSY`，而不是把 busy timeout 等完。
- **后台任务会让路。** 任何需要独占数据库的操作——检查点、索引压缩——都会等到没人
  写入时再做，所以批量导入不会和后台维护打架。
- **插件没有特权。** 它们通过和第三方客户端同一套带权限的 API 访问文献库。

---

## 文档

| | |
| --- | --- |
| [`docs/00-overview.md`](docs/00-overview.md) | 这是什么、给谁用 |
| [`docs/01-architecture.md`](docs/01-architecture.md) | 分层、crate、边界 |
| [`docs/03-data-model.md`](docs/03-data-model.md) | 条目、收藏夹、关系 |
| [`docs/04-api-design.md`](docs/04-api-design.md) | HTTP API |
| [`docs/06-search-and-pdf.md`](docs/06-search-and-pdf.md) | 检索与 PDF 处理 |
| [`docs/08-security-and-deploy.md`](docs/08-security-and-deploy.md) | 威胁模型与打包 |
| [`docs/11-agents.md`](docs/11-agents.md) | 助手及其边界 |
| [`docs/14-storage-layout.md`](docs/14-storage-layout.md) | 磁盘上有什么、在哪 |
| [`docs/16-workspace-rules.md`](docs/16-workspace-rules.md) | 到目前为止犯过的每一个错 |

---

## 许可

AGPL-3.0-or-later，见 [LICENSE](LICENSE)。

简单说：随便用，包括商用；如果你改了它并通过网络提供给别人使用，请公开你的修改。
一个装着某人十年阅读记录的文献管理工具，不该是他自己有可能被锁在门外的东西。
