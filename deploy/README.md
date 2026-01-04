# nBot · 一键部署（Docker Hub / 国内可选加速）

这个目录用于“开源发布 / 仅靠 Docker 部署”的场景：目录顶层只保留 `README.md` + `docker/`，所有脚本都在 `docker/` 目录内，且**严禁在客户机编译**（只 pull 镜像 / 下载预编译包）。

源码仓库在：`https://github.com/yukmakoto/nBot`。

## Linux 一键部署（Docker，推荐）

仅依赖：`sudo` + `curl`（脚本会自动安装 Docker/compose；不会 build）。

注意：当前 DockerHub 镜像仅提供 `linux/amd64`。

一行命令：

- 直连：
  - `curl -fsSL https://raw.githubusercontent.com/yukmakoto/nBot/main/deploy/docker/install-docker.sh | sudo bash`
- 国内加速（推荐）：
  - `curl -fsSL https://gh-proxy.org/https://raw.githubusercontent.com/yukmakoto/nBot/main/deploy/docker/install-docker.sh | sudo bash`

脚本默认安装到 `/opt/nbot`，并生成：

- `/opt/nbot/docker-compose.yml`
- `/opt/nbot/.env`（首次会随机挑选 WebUI/渲染服务端口，避免冲突；后续保持不变）

常用维护命令：

- 更新到最新镜像：`cd /opt/nbot && docker compose pull && docker compose up -d`
- 停止服务：`cd /opt/nbot && docker compose down`
- 查看日志：`cd /opt/nbot && docker compose logs -f bot`

## Windows

Windows 如果也要“容器化 + NapCat 多实例隔离”，请用 Docker Desktop（脚本会自动安装/启用 WSL2，可能会重启电脑；不会 build）。

- 直连：
  - `powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://raw.githubusercontent.com/yukmakoto/nBot/main/deploy/docker/install-docker.ps1 | iex"`
- 国内加速（推荐）：
  - `powershell -NoProfile -ExecutionPolicy Bypass -Command "iwr -useb https://gh-proxy.org/https://raw.githubusercontent.com/yukmakoto/nBot/main/deploy/docker/install-docker.ps1 | iex"`

脚本默认安装到 `C:\ProgramData\nbot`，并生成 `docker-compose.yml` + `.env`（端口首次随机，后续保持不变）。

说明：如果你的系统尚未启用 WSL2/虚拟化组件，Windows 可能需要 **重启一次** 才能生效；脚本会设置“登录后自动续跑”，你不需要再次执行命令。

如果你不想装 Docker（仅做本机轻量运行），才用预编译一键包（Release）`nbot-windows-x86_64.zip`，解压后双击 `start.bat`。

## 关键配置（可选）

这些都可以在执行前通过环境变量覆盖；执行后会写入安装目录的 `.env` 作为持久化配置：

- Linux 默认：`/opt/nbot/.env`
- Windows 默认：`C:\ProgramData\nbot\.env`

安装目录可通过 `NBOT_INSTALL_DIR` 覆盖（Linux/Windows 都支持）。

- `NBOT_DOCKERHUB_NAMESPACE`：镜像命名空间（默认 `yukmakoto`）
- `NBOT_TAG`：镜像 tag（默认 `latest`）
- `NBOT_DOCKER_REGISTRY`：镜像源前缀（默认 `docker.nailed.dev`；如你希望直连 DockerHub 可设为 `docker.io`）
- `NBOT_API_TOKEN`：WebUI 鉴权 token（默认自动生成；会写入安装目录 `.env`，并在脚本结尾打印）
- `NBOT_WEBUI_BIND_HOST`：WebUI 绑定 IP（默认 `0.0.0.0`；如需仅本机访问可设 `127.0.0.1`）
- `NBOT_WEBUI_PUBLIC_HOST`：WebUI 公网访问域名/IP（可选；仅用于脚本输出展示，不参与端口绑定）
- `NBOT_WEBUI_PORT`：WebUI 绑定端口（默认随机）
- `NBOT_RENDER_BIND_HOST`：渲染服务绑定 IP（默认 `127.0.0.1`）
- `NBOT_RENDER_PORT`：渲染服务端口（默认随机）

示例（局域网访问 WebUI）：

- `export NBOT_WEBUI_BIND_HOST=0.0.0.0; curl -fsSL https://raw.githubusercontent.com/yukmakoto/nBot/main/deploy/docker/install-docker.sh | sudo -E bash`

说明：如果你的服务器是云主机（EIP/NAT），脚本可能只能探测到内网 IP（如 `172.*`）；这不影响实际绑定，但你需要用云控制台里的公网 IP 访问，或设置 `NBOT_WEBUI_PUBLIC_HOST=<公网IP/域名>` 让脚本输出正确的访问地址。

## 无交互登录（可选）

如果你的镜像源默认需要鉴权、且你希望脚本全程不提问，可以在执行前提供登录信息（**不要写进仓库**）：

- Linux：`export NBOT_REGISTRY_USERNAME=... NBOT_REGISTRY_PASSWORD=...; curl -fsSL <url> | sudo -E bash`
- Windows：先在 PowerShell 设置 `$env:NBOT_REGISTRY_USERNAME=...; $env:NBOT_REGISTRY_PASSWORD=...` 再执行脚本

## 镜像仓库登录（如需要）

如果你的镜像源需要鉴权（`docker pull` 提示 `unauthorized` / `authentication required`），脚本会提示你输入用户名/密码完成登录：

- Linux：在脚本交互中输入用户名/密码（不会回显）
- Windows：在脚本交互中输入用户名/密码（不会回显）

脚本**不会**把用户名/密码写入 `.env`、也不会打印到终端日志；它只会在安装目录写入一个专用的 `docker-config/config.json`，并将其只读挂载进 `bot` 容器，用于：

- 部署时 `docker compose pull` 拉取镜像
- 运行中由 `bot` 容器内部拉取 NapCat / 工具镜像（避免“宿主机已登录但容器内拉不动”的问题）

请把 `docker-config/config.json` 当作敏感文件：不要上传到任何公共仓库/网盘；如疑似泄漏请及时在镜像仓库侧更换密码/Token。

## 数据持久化

- nBot 数据：使用 named volume `nbot_data`（容器启动时会自动初始化内置数据）
