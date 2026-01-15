# nBot / nbot-site 文档

本文档面向准备上线与二次开发的维护者，按三个板块组织：
1) 快速开始：把 `nbot-site`（官网 + 插件市场）与 `nBot`（Bot + WebUI）跑起来，并完成 Market 对接  
2) 插件开发：JS 插件的目录结构、`manifest.json` 字段、插件运行时 API、可用钩子、打包与验签  
3) 框架建设：Market-first 架构、签名链路、数据目录/卷、升级与运维建议

---

## 1. 快速开始

### 1.1 组件关系（先读这个）

- `nbot-site`：对外提供官网与插件市场 API（source of truth）；负责给插件包签名并提供公钥
- `nBot`：QQ Bot 框架本体 + WebUI；可从 `nbot-site` 拉取官方插件并在本地验签安装
- 对接关键是两项配置：
  - `NBOT_MARKET_URL`：nBot 访问插件市场的 base URL（例如 `https://nbot.nailed.dev`）
  - `NBOT_OFFICIAL_PUBLIC_KEY_B64`：nBot 用于验签的官方公钥（建议直接取 `nbot-site` 的 `GET /api/public-key`）

### 1.2 部署 nbot-site（插件市场）

#### 方式 A：Docker（推荐）

`nbot-site` 仓库内带 `docker-compose.yml`，默认监听 `127.0.0.1:3000`，通常配合反代对外提供 HTTPS：

```bash
cd nbot-site
docker compose up -d
```

常用环境变量（`nbot-site/README.md` 有完整列表）：

- `PORT`：默认 `3000`
- `NBOT_SITE_KEYS_DIR`：默认 `data/keys`（建议持久化卷挂载）
- `NBOT_SITE_BOOTSTRAP_OFFICIAL_PLUGINS=true`：启动时从 `official-plugins/` 自动发布官方插件

#### 方式 B：二进制

```bash
cd nbot-site
cargo build --release
./target/release/nbot-site
```

### 1.3 验证 nbot-site API（必须做）

对外公开 API（来自 `nbot-site/src/main.rs` 路由）：

- `GET /api/stats`：公开统计信息（已审核插件数、官方插件数、下载量、公钥长度、是否启用 admin）
- `GET /api/plugins`：插件列表
- `GET /api/plugins/:id`：插件详情
- `GET /api/plugins/:id/download`：下载插件包（`.nbp`）
- `GET /api/public-key`：获取验签公钥（Base64）

用 `curl` 快速检查：

```bash
curl -fsSL https://<your-nbot-site>/api/plugins | head
curl -fsSL https://<your-nbot-site>/api/public-key
curl -fsSL -o smart-assist.nbp https://<your-nbot-site>/api/plugins/smart-assist/download
```

#### 管理 API（可选）

当 `nbot-site` 设置了 `NBOT_SITE_ADMIN_TOKEN` 时会启用 `/api/admin/*`（未设置则 403）。鉴权方式：

```text
Authorization: Bearer <NBOT_SITE_ADMIN_TOKEN>
```

管理接口（实际实现见 `nbot-site/src/main.rs`）：

- `GET /api/admin/pending`：待审核列表
- `POST /api/admin/approve/:id`：审核通过（会对文件树签名并写入 `manifest.signature`）
- `POST /api/admin/reject/:id`：审核拒绝
- `POST /api/admin/publish`：直接发布（用于官方插件；跳过 pending）

#### 提交审核 API（可选）

`POST /api/plugins/submit`：提交插件进入待审核区。请求 JSON（实际结构见 `nbot-site/src/api/handlers.rs`）：

- `manifest`: object（插件 manifest）
- `code`: string?（单文件插件内容；入口通常为 `index.js`）
- `files`: `{[path: string]: string}`?（多文件插件内容；路径为相对路径）

### 1.4 部署 nBot（Bot + WebUI）

#### 方式 A：一键脚本（Linux）

参考 `nBot/README.md` 的一键安装脚本，默认安装到 `/opt/nbot`，并以 Docker 方式运行（只拉取镜像、不在客户机编译）。

#### 方式 B：docker-compose

nBot Docker 运行时关键点：

- `NBOT_DATA_DIR` 通常指向容器内 `/app/data`（由 Docker volume 持久化）
- WebUI 与后端共用同一地址（WebUI 静态文件由后端 `dist/` 提供）

### 1.5 获取并使用 nBot API Token

nBot 的 `/api/*` 默认需要鉴权（实现：`nBot/backend/src/auth.rs`）。token 来源优先级：

1) 环境变量 `NBOT_API_TOKEN`  
2) `data/state/api_token.txt`（首次启动会生成）

调用示例：

```bash
TOKEN="$(cat data/state/api_token.txt)"
curl -fsSL -H "Authorization: Bearer $TOKEN" http://127.0.0.1:32100/api/system/info
curl -fsSL -H "Authorization: Bearer $TOKEN" http://127.0.0.1:32100/api/plugins/installed
```

### 1.6 对接插件市场（Market-first）

nBot 侧接口与行为（源码：`nBot/backend/src/plugin_handlers/market.rs`、`nBot/backend/src/plugin/registry.rs`）：

- 当 `NBOT_MARKET_URL` 非空时，nBot 默认不再使用 seed 内置插件（镜像里自带的 `data.seed/plugins`）
- 启动时会自动同步官方插件集（默认开）：`NBOT_MARKET_BOOTSTRAP_OFFICIAL_PLUGINS=true`
- 官方插件包必须有签名（`manifest.signature`），否则安装会失败（除非开发环境开启 `NBOT_ALLOW_UNSIGNED_PLUGINS=true`）

你需要在 nBot 运行环境设置：

```bash
NBOT_MARKET_URL=https://<your-nbot-site>
NBOT_OFFICIAL_PUBLIC_KEY_B64=<output of https://<your-nbot-site>/api/public-key>
```

验证（nBot 的后端 API 前缀均为 `/api`）：

- `GET /api/market/plugins`：查看 Market 插件列表（从 nbot-site 转发）
- `POST /api/market/sync`：同步官方插件（安装/更新；保留配置与启用状态）
- `GET /api/plugins/installed`：查看本地已安装插件

### 1.7 生产升级（不在 VPS 编译）

生产环境建议始终走“拉镜像 + 重启容器”的方式（不要在 VPS 上编译）。

以默认安装目录 `/opt/nbot` 为例：

```bash
cd /opt/nbot
export NBOT_TAG=v0.0.6
docker pull docker.nailed.dev/yukmakoto/nbot-bot:$NBOT_TAG
docker pull docker.nailed.dev/yukmakoto/nbot-render:$NBOT_TAG
docker compose up -d --remove-orphans
docker compose ps
```

#### docker pull 很慢/卡住怎么处理

1) **确认镜像存在**（避免“拉不存在的 tag”浪费时间）

```bash
curl -fsSL -H 'Accept: application/vnd.docker.distribution.manifest.v2+json' \
  https://docker.nailed.dev/v2/yukmakoto/nbot-bot/manifests/$NBOT_TAG >/dev/null
```

2) **Cloudflare 路径慢**：可临时让机器直连源站（只建议临时使用；完成后恢复）

```bash
echo '<origin_ip> docker.nailed.dev # nbot-registry-bypass' >> /etc/hosts
```

完成后清理：

```bash
tmp=$(mktemp); awk '!/nbot-registry-bypass/' /etc/hosts > "$tmp"; cat "$tmp" > /etc/hosts; rm -f "$tmp"
```

3) **containerd layer 锁死导致一直“没进度”**：日志会出现类似

```text
layer-sha256:<digest> locked for ...: unavailable
```

这通常是一次失败的拉取留下的锁。处理方式：

```bash
systemctl restart containerd docker
```

重启后再 `docker pull` 一次即可。

---

## 2. 插件开发

### 2.1 插件目录结构（运行态）

nBot 的插件安装位置在数据目录下（默认 `NBOT_DATA_DIR=data`，Docker 常用 `/app/data`）：

- `data/plugins/bot/<pluginId>/...`
- `data/plugins/platform/<pluginId>/...`
- 状态文件：`data/state/plugins.json`
- 插件存储：`data/storage/*.json`（由 `nbot.storage.*` 管理）

### 2.2 manifest.json 字段（以实际实现为准）

后端结构体定义在 `nBot/backend/src/plugin/types.rs`，字段使用 `camelCase`：

必填字段：
- `id`: string（建议 `[A-Za-z0-9_.-]`，最长 64；后端会校验）
- `name`: string
- `version`: string（例如 `1.0.0`）
- `author`: string
- `description`: string
- `type`: `"bot"` | `"platform"`

可选字段（常用）：
- `entry`: string（默认 `index.js`；也可以是目录，运行时会尝试 `<entry>/index.js`）
- `codeType`: `"script"` | `"module"`
  - `script`：兼容旧写法，入口允许顶层 `return { ... }`
  - `module`：ESM 模块，入口应 `export default { ... }`
- `commands`: string[]（插件提供的命令名）
- `configSchema`: 表单 schema（用于 WebUI 配置 UI）
- `config`: object（运行时配置会写回 manifest；签名不会覆盖 manifest）
- `signature`: string | null（Base64；官方/市场分发插件必须有）
- `builtin`: boolean（内置/官方标记；Market 分发通常为 `false`）

### 2.3 插件钩子（Plugin Hooks）

插件本体对象挂在 `globalThis.__plugin`，由运行时按钩子名调用（实现：`nBot/backend/src/plugin/runtime.rs`）。

支持的钩子（按调用时机）：

- `onConfigUpdated(newConfig)`：配置变更热更新回调（兼容旧名 `updateConfig(newConfig)`）
- `preCommand(ctx) -> boolean|void`：命令执行前；返回 `false` 可阻止执行
- `preMessage(ctx) -> boolean|void`：消息处理前；返回 `false` 可阻止后续处理
- `onCommand(ctx)`：执行插件命令
- `onNotice(ctx) -> boolean|void`：通知事件；返回 `false` 可阻止
- `onMetaEvent(ctx) -> boolean|void`：meta_event；返回 `false` 可阻止
- `onLlmResponse({requestId, success, content})`：异步 LLM 回调
- `onGroupInfoResponse({requestId, infoType, success, data})`：异步群信息/文件/下载回调

### 2.4 JS 运行时 API（globalThis.nbot）

插件 SDK 位于 `nBot/backend/src/plugin/js/runtime.js`。常用 API：

消息与 OneBot：
- `nbot.at(userId) -> string`
- `nbot.sendMessage(groupId, content)`
- `nbot.sendReply(userId, groupId, content)`
- `nbot.callApi(action, params)`

LLM 调用（部分为异步回调到 `onLlmResponse`）：
- `nbot.callLlmForward(...)`
- `nbot.callLlmForwardFromUrl(...)`
- `nbot.callLlmForwardArchiveFromUrl(...)`（支持 `.zip/.tar/.tar.gz/.gz`）
- `nbot.callLlmForwardImageFromUrl(...)`
- `nbot.callLlmForwardVideoFromUrl(...)`
- `nbot.callLlmForwardAudioFromUrl(...)`
- `nbot.callLlmForwardMediaBundle(...)`
- `nbot.callLlmChat(requestId, messages, options)`
- `nbot.callLlmChatWithSearch(requestId, messages, options)`

渲染与网络：
- `nbot.httpFetch(url, timeoutMs)`
- `nbot.renderMarkdownImage(title, meta, markdown, width)`
- `nbot.renderHtmlImage(html, width, quality)`

配置与存储：
- `nbot.getConfig()`
- `nbot.setConfig(obj)`
- `nbot.storage.get/set/delete(key)`

群/好友/文件（异步回调到 `onGroupInfoResponse`）：
- `nbot.fetchGroupNotice(requestId, groupId)`
- `nbot.fetchGroupMsgHistory(requestId, groupId, options)`
- `nbot.fetchGroupFiles(requestId, groupId, folderId?)`
- `nbot.fetchGroupFileUrl(requestId, groupId, fileId, busid?)`
- `nbot.fetchFriendList(requestId)`
- `nbot.fetchGroupList(requestId)`
- `nbot.fetchGroupMemberList(requestId, groupId)`
- `nbot.downloadFile(requestId, url, options)`

### 2.5 最小示例插件

#### 示例 1：`script` 模式（单文件）

目录：`data/plugins/bot/hello-script/`

`manifest.json`（示例）：

```json
{
  "id": "hello-script",
  "name": "Hello Script",
  "version": "0.0.1",
  "author": "you",
  "description": "Minimal script plugin demo",
  "type": "bot",
  "entry": "index.js",
  "codeType": "script",
  "commands": ["hello"],
  "configSchema": [
    { "key": "enabled", "type": "boolean", "label": "启用", "default": true }
  ],
  "config": { "enabled": true }
}
```

`index.js`（示例）：

```js
return definePlugin({
  preMessage(ctx) {
    const cfg = nbot.getConfig() || {};
    if (!cfg.enabled) return true;
    return true;
  },
  onCommand(ctx) {
    // ctx.content / ctx.group_id / ctx.user_id 由运行时传入（JSON）
    nbot.sendReply(ctx.user_id, ctx.group_id, nbot.at(ctx.user_id) + " hello!");
  },
});
```

#### 示例 2：`module` 模式（可多文件）

`manifest.json` 里将 `codeType` 设为 `module`，并在入口文件中 `export default { ... }`。

### 2.6 打包与安装（.nbp）

nBot 原生插件包格式由 `nBot/backend/src/plugin/package.rs` 定义：

- `.nbp` = `tar.gz`
- 必须包含 `manifest.json`
- 允许包含任意文件树（多文件/目录插件）

提供了一个打包脚本（用于开发/测试，不负责签名）：

```bash
python nBot/scripts/pack_nbp.py --src path/to/plugin-dir --out plugin.nbp
```

nBot 后端安装接口：

- `POST /api/plugins/package`：上传 base64 的 `.nbp` 安装
- `POST /api/plugins/install`：上传 `{manifest, code}` 安装（单文件）

签名校验要点（实现：`nBot/backend/src/plugin_handlers/install.rs`）：

- 官方/市场分发插件要求 `manifest.signature` 存在且有效
- 验签消息基于包内文件树（不包含 `manifest.json`，避免配置写回导致签名失效）

---

## 3. 框架建设

### 3.1 Market-first（插件市场为准）

当 `NBOT_MARKET_URL` 非空时：

- nBot 默认禁用 seed 内置插件（实现：`nBot/backend/src/plugin/registry.rs`）
- 启动会同步官方插件集合（实现：`nBot/backend/src/plugin_handlers/market.rs`）
- WebUI 可手动触发同步：`POST /api/market/sync`

### 3.2 签名链路（ed25519）

- `nbot-site` 持有私钥，对官方插件包签名并对外发布公钥（`GET /api/public-key`）
- `nBot` 仅持有公钥（`NBOT_OFFICIAL_PUBLIC_KEY_B64`），负责在安装/更新时验签
- 生产建议：不要在 nBot 端开启 `NBOT_ALLOW_UNSIGNED_PLUGINS=true`

### 3.3 数据目录与 Docker 卷（非常重要）

Docker 部署下，nBot 的数据通常在名为 `nbot_nbot_data` 的 Docker volume 中：

- 容器内路径：`/app/data`
- 宿主机路径（Docker）：`/var/lib/docker/volumes/nbot_nbot_data/_data`

运维操作（例如清理历史内置插件）一定要对真实数据卷操作，而不是删 `/opt/nbot/data` 这种“看起来像目录”的路径。

### 3.4 对外接口总览（常用）

鉴权（实现：`nBot/backend/src/auth.rs`）：

- `Authorization: Bearer <NBOT_API_TOKEN>`
- 或 Cookie：`nbot_session=<token>`

#### nBot API 路由速查（来自 `nBot/backend/src/main.rs`）

```text
GET /api/status
GET /api/system/stats
GET /api/system/info
GET /api/system/logs
GET /api/system/export
GET /api/docker/info
GET /api/message/stats
POST /api/bots
GET /api/bots/list
GET /api/bots/:id
DELETE /api/bots/:id
PUT /api/bots/:id
PUT /api/bots/:id/discord
POST /api/bots/:id/login
POST /api/bots/:id/copy
GET /api/bots/:id/modules
PUT /api/bots/:id/module
GET /api/bots/:id/module/:module_id
DELETE /api/bots/:id/module/:module_id
GET /api/napcat/qr
DELETE /api/napcat/qr
GET /api/tasks
DELETE /api/tasks/:id
GET /api/docker/list
POST /api/docker/action
GET /api/docker/logs
GET /api/databases
POST /api/databases
DELETE /api/databases/:id
POST /api/bots/link-database
GET /api/plugins/installed
POST /api/plugins/install
POST /api/plugins/package
POST /api/plugins/sign
DELETE /api/plugins/:id
POST /api/plugins/:id/enable
POST /api/plugins/:id/disable
POST /api/plugins/:id/config
GET /api/market/plugins
POST /api/market/install
POST /api/market/sync
GET /api/modules
GET /api/modules/:id
POST /api/modules/:id/enable
POST /api/modules/:id/disable
PUT /api/modules/:id/config
GET /api/llm/config
PUT /api/llm/config
POST /api/llm/test
POST /api/llm/models
POST /api/llm/chat
POST /api/llm/tavily/test
GET /api/commands
POST /api/commands
GET /api/commands/:id
PUT /api/commands/:id
DELETE /api/commands/:id
GET /api/tools
POST /api/tools/:id/start
POST /api/tools/:id/stop
POST /api/tools/:id/restart
POST /api/tools/:id/recreate
POST /api/tools/:id/pull
GET /api/relations/friends
GET /api/relations/groups
GET /api/relations/group-members
GET /api/relations/login-info
GET /api/chat/history
POST /api/chat/send
```

#### nbot-site 公开接口

- `GET /api/stats`
- `GET /api/plugins`
- `GET /api/plugins/:id`
- `GET /api/plugins/:id/download`
- `GET /api/public-key`

### 3.5 生产部署建议（落地项）

- `nbot-site`
  - 强制 HTTPS（反代层做 TLS）
  - `data/keys` 必须持久化（否则重启换密钥会导致 nBot 端验签失败）
  - `official-plugins/` 用于自动发布官方插件，减少人工操作
- `nBot`
  - 设置固定 `NBOT_API_TOKEN`，并妥善保存
  - 配置 `NBOT_MARKET_URL` + `NBOT_OFFICIAL_PUBLIC_KEY_B64`，并关闭 `NBOT_ALLOW_UNSIGNED_PLUGINS`
  - 多 QQ 实例时关注 NapCat 容器资源占用（CPU/内存/磁盘）
