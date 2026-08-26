# 10 · 开源许可证与合规

> ⚠️ 本文是工程视角的风险梳理，**不构成法律意见**。正式发布前请以各仓库 `LICENSE` 文件原文为准，并咨询法务。

## 1. 关键第三方组件

| 组件 | 用途 | 许可证（需以仓库原文复核） | 风险 |
| --- | --- | --- | --- |
| `zotero/zotero` 主程序 | 参考实现（**不复制代码**） | AGPL-3.0 | 🔴 只借鉴设计，不抄代码 |
| `zotero/translators` | 站点抓取脚本 | AGPL-3.0 | 🔴 见 §2 |
| `zotero/reader` | PDF 阅读器 | AGPL-3.0 | 🔴 自研阅读器规避 |
| `citeproc-js` | CSL 渲染引擎 | 双授权（CPAL-1.0 / AGPL-3.0；另有说法为 Apache-2.0，**必须核对仓库 LICENSE**） | 🟠 见 §3 |
| `citeproc-rs` | Rust CSL 引擎 | MPL-2.0 | 🟢 文件级 copyleft，可用 |
| CSL `styles` / `locales` | 引用样式与语言包 | CC BY-SA 3.0 | 🟡 分发需署名 + 同方式共享（对样式文件本身） |
| `pdf.js` | 前端 PDF 渲染 | Apache-2.0 | 🟢 |
| `pdfium` | 服务端 PDF | BSD-3-Clause | 🟢 |
| MuPDF | （备选，**不采用**） | AGPL-3.0 / 商业 | 🔴 规避 |
| Tantivy / lindera | 检索与分词 | MIT | 🟢 |
| QuickJS / `rquickjs` | JS 沙箱 | MIT | 🟢 |
| Tauri | 桌面壳 | MIT / Apache-2.0 | 🟢 |
| React / Vite / Tailwind | 前端 | MIT | 🟢 |
| SQLite | 数据库 | Public Domain | 🟢 |
| TipTap | 富文本 | MIT（部分 Pro 扩展商业） | 🟡 只用开源部分 |

## 2. AGPL 与 translators 的核心问题

Zotero 的 translators 是 AGPL-3.0。风险点：

- **在服务端执行 AGPL 脚本，并通过网络提供服务** → AGPL 第 13 条要求向用户提供**"整体作品"**的源码。是否构成"整体作品"取决于结合方式，法律上存在灰度，但保守解读风险高。
- 分发 translators 文件本身（哪怕不改）也需遵守 AGPL 的分发条款。

### 三条可行路线

| 路线 | 做法 | 后果 |
| --- | --- | --- |
| **L1（推荐）：项目本体 AGPL-3.0** | Yinkote Server + Web 全部以 AGPL-3.0 开源，自由复用 translators 与 CSL 生态 | 最省事，社区友好；但阻断闭源商业分发，SaaS 化也须开源 |
| **L2：核心宽松 + AGPL 组件可选下载** | 本体 Apache-2.0/MIT，不内置 translators；用户在设置里**主动选择**从上游安装（类似浏览器装扩展），沙箱通过稳定接口调用 | 商业友好度高，但"是否仍构成结合作品"仍有解释空间，需法务确认；且用户体验多一步 |
| **L3：完全自研抓取层** | 只用官方开放 API（Crossref、OpenAlex、PubMed、arXiv、DataCite、Semantic Scholar、OpenLibrary）+ 自研 HTML 解析器 + Schema.org/Highwire/DC/OG meta 通用抽取 | 零 AGPL 风险，覆盖率初期低但可持续增长；**长期最健康** |

**建议**：**L1 起步（AGPL-3.0），同时按 L3 持续建设自研抓取层**。
理由：这类工具的护城河在生态与信任，开源本身就是获客手段；而自研抓取层既降低法律依赖，又提升质量（官方 API 比爬 HTML 稳定得多）。若未来需要商业版，届时抓取层已可独立，再切换到 L2/L3。

## 3. citeproc 的选择

- 引用输出必须**与 Zotero 逐字一致**，否则用户不敢用 —— 这强烈指向 `citeproc-js`（CSL 事实标准实现）。
- 若其许可证经核实为 AGPL/CPAL 双授权，则在 L1 路线下（本体 AGPL）无障碍；在 L2/L3 路线下需改用 **`citeproc-rs`（MPL-2.0）**，并接受其样式覆盖度与成熟度较低的现实。
- 无论选哪个，都用 **CSL 官方测试套件**做回归，保证正确性可度量。

## 4. CSL 样式（CC BY-SA 3.0）

- 分发内置样式时需保留署名与许可声明；对样式文件的**修改**需以相同方式共享。
- Yinkote 的做法：`resources/styles/` 内附 `LICENSE` 与 `AUTHORS`，UI 的样式详情页展示原作者；对样式的定制（如 GB/T 7714 增强版）以独立仓库 CC BY-SA 3.0 开源回馈社区。

## 5. Zotero Connector 兼容层

实现"兼容协议"（报文形状）通常属于接口互操作，风险低于复制代码。但仍须：
- **不复制** Zotero 任何源码，仅依据公开文档与抓包实现；
- 不使用 "Zotero" 商标做产品名/图标，仅在文档中做"兼容性说明"式的指涉性使用；
- 默认关闭该端口，由用户显式开启。

## 6. 商标与命名

- 产品名 `Yinkote` / 中文名"引可特" —— 发布前做商标检索。
- 图标、配色不得与 Zotero 近似。
- 文案避免"Zotero 替代品"式的商标性表述，改用"支持从 Zotero 迁移"。

## 7. 结论（建议采纳）

```
Yinkote 采用 AGPL-3.0 许可证。
├─ 内置：translators（AGPL，署名保留）、citeproc-js、CSL styles（CC BY-SA 署名）
├─ 自研：抓取层持续建设，目标 2 年内官方 API + 自研解析覆盖 80% 高频场景
├─ 商业化（若需）：Hub 同步服务 / 企业支持 / 托管，而非闭源客户端
└─ 所有第三方许可证在「关于」页面与 THIRD-PARTY-LICENSES.md 中完整披露
```
