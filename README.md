<p align="center">
  <img src="assets/nbot_logo.png" alt="nBot" width="220" />
</p>

<p align="center">
  <a href="https://github.com/yukmakoto/nBot/actions/workflows/ci.yml"><img alt="Pipeline" src="https://github.com/yukmakoto/nBot/actions/workflows/ci.yml/badge.svg" /></a>
</p>

<p align="center">
  带 WebUI 的 QQ Bot 框架（Rust/Axum + React WebUI）。默认通过 NapCat（OneBot 11）接入 QQ，支持插件化扩展与工具容器。
</p>

## 一键部署（Linux）

只依赖：`sudo` + `curl`。脚本会交互式选择主程序运行方式：
- `docker`（推荐）：只会 **pull 镜像**（不会在客户机编译）
- `host`：下载 **预编译 release**（不会在客户机编译）

当前 DockerHub 镜像仅提供 `linux/amd64`。

默认镜像源使用 `docker.nailed.dev`（自建 DockerHub 加速/缓存）；如需直连 DockerHub 可设 `NBOT_DOCKER_REGISTRY=docker.io`。

- 直连：
  - `curl -fsSL https://raw.githubusercontent.com/yukmakoto/nBot/main/deploy/docker/install-docker.sh | sudo bash`
- 国内加速（推荐）：
  - `curl -fsSL https://gh-proxy.org/https://raw.githubusercontent.com/yukmakoto/nBot/main/deploy/docker/install-docker.sh | sudo bash`

默认安装到 `/opt/nbot`，并会在首次运行时随机选择 WebUI/渲染端口避免冲突。

## 系统要求（Linux / Docker）

- 架构：当前 DockerHub 镜像仅提供 `linux/amd64`
- CPU：建议 ≥ 2 核（插件/渲染/多实例会更吃 CPU）
- 内存：主程序本体占用通常不高；真正吃内存的是 NapCat（QQ）实例。单实例最低建议 ≥ 2GB，多实例/插件较多建议 ≥ 4GB（更稳）
- 磁盘（建议值，越多越好）：
  - 首次拉取镜像下载量约 **0.55GB**（压缩层，`nbot-bot`≈270MB + `nbot-render`≈294MB，且部分层可复用）
  - 镜像解压落盘通常会显著大于下载量；再加上数据卷（运行态 state + NapCat 数据），最低建议 **5GB**，日常建议 **10GB**，多号/长期运行建议 **20GB+**
- 网络：需要能访问镜像仓库（国内可通过 `NBOT_DOCKER_REGISTRY` 配置镜像加速源）

## 主要功能

- WebUI：机器人/模块/插件管理与可视化
- 插件系统：JS 插件运行时（可选签名校验）
- 工具容器：`wkhtmltoimage`（HTML → 图片，用于帮助/报告等渲染）
- NapCat 多实例：每个 QQ 独立容器/数据隔离（适合多号）

## 插件开发（JS）

插件目录示例：`data/plugins/bot/<pluginId>/`。

- `manifest.json`：插件元信息与配置 schema（运行时配置会写回这里）
- 入口文件：由 `manifest.json` 的 `entry` 指定（默认 `index.js`）
- 入口加载方式：由 `manifest.json` 的 `codeType` 指定：
  - `script`（默认）：兼容旧写法，入口文件可用顶层 `return { ... }`
  - `module`：ESM 入口，可用 `import`/多文件目录结构，需 `export default { ... }`
- 安装包（`.nbp`）：支持打包整个目录树（不仅限 `index.js`）；签名校验基于包内文件树（不包含 `manifest.json`，避免用户配置写回导致签名失效）

## 官方插件市场（nbot-site）

当配置了 `NBOT_MARKET_URL` 时，nBot 会默认 **不再从镜像内置的 seed 插件目录同步插件**，而是以市场（`nbot-site`）为准：

- 首次启动（仅当本地没有任何已安装插件）会自动从 Market 安装官方插件集
- 插件包签名校验使用 `NBOT_OFFICIAL_PUBLIC_KEY_B64`（建议设置为 `nbot-site` 的 `/api/public-key`）

相关环境变量：

- `NBOT_MARKET_URL`：插件市场地址（例如 `https://nbot.nailed.dev`）
- `NBOT_MARKET_BOOTSTRAP_OFFICIAL_PLUGINS`：是否在启动时自动同步官方插件（安装/更新；默认 `true`）
- `NBOT_MARKET_FORCE_UPDATE`：是否强制重新安装官方插件（默认 `false`；一般不需要）
- `NBOT_USE_SEED_BUILTIN_PLUGINS`：强制继续使用 seed 内置插件（默认 `false`）
- `NBOT_DISABLE_SEED_BUILTIN_PLUGINS`：强制禁用 seed 内置插件（默认：当 `NBOT_MARKET_URL` 非空时启用）

## 目录结构

- `backend/`：后端服务（API、机器人运行时、插件系统）
- `webui/`：WebUI（React + Vite + Tauri）
- `assets/`：渲染模板与静态资源
- `data/`：内置模块/插件/指令定义（不包含运行态 state）
- `deploy/`：一键部署（仅 `README.md` + `docker/`）

## API Token（默认开启）

首次启动会在 `data/state/api_token.txt` 生成 token，也可以通过环境变量 `NBOT_API_TOKEN` 指定。

## 本地开发

依赖：Rust（>= 1.88）、Node.js（>= 20）、Docker。

- Windows：`powershell -ExecutionPolicy Bypass -File scripts/dev.ps1`
- Linux/macOS：`bash scripts/dev.sh`
- WebUI（Vite）：`cd webui && npm install && npm run dev`（需同时启动后端）
- Desktop（Tauri）：`cd webui && npm install && npm run tauri dev`（默认连 `http://127.0.0.1:32100`，可在登录页改后端地址）

## 发布（维护者）

- `ci.yml`：push/PR 会自动跑 `cargo check/test`；打 tag（`v*`）会同时发布 GitHub Release + 推送 DockerHub 镜像。
  - GitHub Release：Windows/Linux 预编译包（Releases）。
  - DockerHub：`nbot-bot` / `nbot-render`（需要 repo secrets：`DOCKERHUB_USERNAME`、`DOCKERHUB_TOKEN`）。

更多部署说明见 `deploy/README.md`。

## License

本项目使用 `GPL-3.0-only`：允许私用/商用，但任何分发都必须开源并保持同许可（禁止闭源分发）。详见 `LICENSE`。
