# Yinkote 插件开发

一个插件就是一个目录，里面放一份 `plugin.json`，加上任意语言写的可执行程序。

```
plugins/
└─ my-plugin/
   ├─ plugin.json      # 清单：身份、运行时、能力、权限、钩子
   └─ main.mjs         # 入口（任何语言都行）
```

把目录放进 `<data-dir>/plugins/`，或用 `yinkote --plugin-dir ./plugins` 指定额外目录，然后在工作台的「插件」面板点「重新扫描」。

## 1. 清单 `plugin.json`

```jsonc
{
  "id": "crossref",              // [A-Za-z0-9._-]，全局唯一
  "name": "Crossref 元数据",
  "version": "1.0.0",
  "description": "…",
  "apiVersion": 1,               // 必须与宿主一致，否则拒绝加载
  "runtime": {
    "type": "process",           // process | builtin
    "command": "node",           // 相对路径会在插件目录内解析
    "args": ["main.mjs"],
    "env": { "FOO": "bar" }
  },
  "capabilities": ["metadata_source"],   // 供 UI 描述，见下表
  "permissions": ["items_write", "settings"],
  "hooks": ["item.beforeCreate"],
  "enabled": true,
  "timeoutMs": 20000             // 单次调用超时
}
```

| capability | 含义 |
| --- | --- |
| `metadata_source` | 从外部服务检索文献元数据 |
| `importer` / `exporter` | 解析 / 生成某种文件格式 |
| `search_provider` | 贡献额外的检索结果 |
| `item_action` | 在条目上添加右键动作 |
| `hook` | 订阅生命周期事件 |

## 2. 协议

**换行分隔的 JSON-RPC 2.0，走 stdin/stdout。** 一行一条消息。
`stderr` 会被宿主当作日志采集 —— **不要往 stderr 写协议数据**。

协议是**双向**的：宿主调用插件，插件也可以回调宿主。

### 宿主 → 插件

| 方法 | 参数 | 返回 |
| --- | --- | --- |
| `initialize` | `{apiVersion, hostVersion, pluginId, permissions}` | `{contributions: {...}}` |
| `hook` | `{name, payload}` | 见 §4 |
| `shutdown` | `null` | `null`，随后退出 |
| 任意自定义方法 | 由 `POST /api/v1/plugins/{id}/call` 转发 | 任意 JSON |

`initialize` 返回的 `contributions` 就是运行期注册：

```json
{
  "contributions": {
    "metadataSources": [
      { "id": "crossref", "label": "Crossref", "supports": ["query", "doi"] }
    ],
    "importers":  [{ "id": "bibtex", "label": "BibTeX", "extensions": ["bib"] }],
    "exporters":  [],
    "itemActions": [{ "id": "open-doi", "label": "打开 DOI", "itemTypes": [] }]
  }
}
```

### 插件 → 宿主

插件在同一条流上发出**自己的**请求即可（用不与宿主冲突的 id）：

```json
{"jsonrpc":"2.0","id":100001,"method":"host.items.create","params":{"items":[...]}}
```

| 宿主方法 | 需要权限 | 说明 |
| --- | --- | --- |
| `host.version` | — | 宿主版本 |
| `host.log` | — | 写入宿主日志 |
| `host.notify` | `notify` | 向所有 UI 客户端推送提示 |
| `host.items.search` | `search` | 混合检索 `{q, mode, limit}` |
| `host.items.list` | `items_read` | 列出条目 |
| `host.items.get` | `items_read` | 按 key 取条目 |
| `host.items.create` | `items_write` | 批量创建 `{items: [draft]}` |
| `host.collections.list` | `collections_read` | 收藏夹列表 |
| `host.tags.list` | `items_read` | 标签列表 |
| `host.settings.get` / `.set` | `settings` | 插件私有键值（自动按插件 id 命名空间隔离） |

权限不足会返回 JSON-RPC 错误码 `-32000`。

## 3. 权限模型的边界（重要）

权限门控的是**宿主 API**，不是操作系统。进程型插件本来就能自己开网络连接、读文件 ——
`process` 运行时提供的是**故障隔离**（崩溃、死循环、内存泄漏不会拖垮宿主），**不是沙箱**。

因此：
- `network` 权限是**声明性**的，用于让用户在安装时知情，宿主不会也无法拦截插件自己发起的连接；
- 请只安装你信任来源的插件，就像对待任何本机软件一样；
- 需要真正沙箱化时，未来的 `wasm` 运行时会提供强制隔离。

## 4. 钩子

| 钩子 | 触发时机 | 返回值的作用 |
| --- | --- | --- |
| `startup` | 服务启动后 | 忽略 |
| `shutdown` | 服务退出前 | 忽略 |
| `item.beforeCreate` | 条目**写库之前** | `{fields: {...}, tags: ["x"]}` —— `fields` 仅填补**缺失**字段，`tags` 追加为自动标签 |
| `item.created` | 写库之后（异步） | 忽略 |
| `item.updated` | 更新之后（异步） | 忽略 |
| `item.trashed` | 移入回收站之后（异步） | 忽略 |
| `search.rerank` | 预留 | — |

`item.beforeCreate` 是唯一能**改变**结果的钩子，它在事务之前同步执行，所以务必快
（示例插件 `auto-tag` 用纯正则，耗时 < 1ms）。

## 5. 生命周期与容错

- **懒启动**：停用的插件不会被 spawn。
- **崩溃自愈**：进程挂掉后，下一次调用会自动重启一次；仍失败则标记 `failed`。
- **超时**：超过 `timeoutMs` 的调用被中止，插件被隔离而不是拖住请求。
- **热重载**：`POST /api/v1/plugins/reload` 重新扫描目录并重启变更的插件。
- **停用记忆**：用户停用的插件会写入设置，重启后依然停用。
- **坏清单不致命**：一个插件的清单写错只会出现在 `diagnostics`，不影响其它插件。

## 6. 最小示例

```js
import { createInterface } from 'node:readline'

const send = (m) => process.stdout.write(JSON.stringify(m) + '\n')

const methods = {
  initialize: () => ({ contributions: {} }),
  hook: ({ name, payload }) =>
    name === 'item.beforeCreate' && /survey/i.test(payload?.item?.fields?.title ?? '')
      ? { tags: ['survey'] }
      : {},
  shutdown: () => null,
}

createInterface({ input: process.stdin }).on('line', (line) => {
  const msg = JSON.parse(line)
  const fn = methods[msg.method]
  if (!fn) return send({ jsonrpc: '2.0', id: msg.id, error: { code: -32601, message: 'no method' } })
  send({ jsonrpc: '2.0', id: msg.id, result: fn(msg.params ?? {}) ?? null })
  if (msg.method === 'shutdown') process.exit(0)
})
```

## 7. 随附示例

| 插件 | 演示内容 |
| --- | --- |
| [`crossref/`](crossref/) | 元数据源、自定义方法、回调 `host.items.create` |
| [`auto-tag/`](auto-tag/) | `item.beforeCreate` 钩子改写待写入的条目 |

试一下：

```bash
yinkote --plugin-dir ./plugins

# 用 DOI 查元数据
curl -s -X POST localhost:23130/api/v1/plugins/crossref/call \
  -H 'Content-Type: application/json' \
  -d '{"method":"search","params":{"text":"10.1038/nature14539"}}' | jq

# 直接查并入库
curl -s -X POST localhost:23130/api/v1/plugins/crossref/call \
  -H 'Content-Type: application/json' \
  -d '{"method":"import","params":{"text":"attention is all you need","limit":3}}' | jq
```
