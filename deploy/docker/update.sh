#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
#                              nBot Updater
#                    QQ Bot Framework - One-Click Update
# ============================================================================

# ANSI Color Codes
readonly RED='\033[0;31m'
readonly GREEN='\033[0;32m'
readonly YELLOW='\033[1;33m'
readonly BLUE='\033[0;34m'
readonly MAGENTA='\033[0;35m'
readonly CYAN='\033[0;36m'
readonly WHITE='\033[1;37m'
readonly GRAY='\033[0;90m'
readonly BOLD='\033[1m'
readonly DIM='\033[2m'
readonly NC='\033[0m' # No Color

# Unicode symbols
readonly CHECK="${GREEN}✓${NC}"
readonly CROSS="${RED}✗${NC}"
readonly ARROW="${CYAN}➜${NC}"
readonly INFO="${BLUE}ℹ${NC}"
readonly WARN="${YELLOW}⚠${NC}"
readonly ROCKET="${MAGENTA}🚀${NC}"

print_banner() {
  echo -e "${CYAN}"
  cat << 'EOF'
                ____        __
      ____     / __ )____  / /_
     / __ \   / __  / __ \/ __/
    / / / /  / /_/ / /_/ / /_
   /_/ /_/  /_____/\____/\__/

   QQ Bot Framework - Update Script
EOF
  echo -e "${NC}"
  echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
  echo
}

step() {
  echo -e "${ARROW} ${BOLD}$*${NC}"
}

success() {
  echo -e "${CHECK} ${GREEN}$*${NC}"
}

warn() {
  echo -e "${WARN} ${YELLOW}$*${NC}"
}

info() {
  echo -e "${INFO} ${BLUE}$*${NC}"
}

die() {
  echo -e "${CROSS} ${RED}ERROR: $*${NC}" >&2
  exit 1
}

have() { command -v "$1" >/dev/null 2>&1; }

print_box() {
  local title="$1"
  shift
  echo -e "${CYAN}╭─ ${BOLD}${title}${NC} ${CYAN}─────────────────────────────────────────────────────────────────╮${NC}"
  for line in "$@"; do
    echo -e "${CYAN}│${NC} $line"
  done
  echo -e "${CYAN}╰──────────────────────────────────────────────────────────────────────────────╯${NC}"
}

# Clear screen and show banner
clear 2>/dev/null || true
print_banner

if [[ "${EUID:-$(id -u)}" -ne 0 ]]; then
  die "请用 root 执行（示例：sudo bash update.sh）。"
fi

INSTALL_DIR="${NBOT_INSTALL_DIR:-/opt/nbot}"
ENV_FILE="${INSTALL_DIR}/.env"
DOCKER_CONFIG_DIR="${INSTALL_DIR}/docker-config"

if [[ ! -f "${ENV_FILE}" ]]; then
  die "未找到 ${ENV_FILE}，请先运行安装脚本。"
fi

# Load existing config
set -a
source "${ENV_FILE}"
set +a

export DOCKER_CONFIG="${DOCKER_CONFIG_DIR}"

env_get() {
  local key="$1"
  local file="$2"
  [[ -f "$file" ]] || return 1
  grep -E "^[[:space:]]*${key}=" "$file" | tail -n 1 | sed -E "s/^[[:space:]]*${key}=//"
}

NBOT_MAIN_MODE="${NBOT_MAIN_MODE:-docker}"
NBOT_TAG="${NBOT_TAG:-latest}"

info "安装目录: ${INSTALL_DIR}"
info "部署模式: ${NBOT_MAIN_MODE}"
info "当前标签: ${NBOT_TAG}"
echo

# ============================================================================
# Update Docker images
# ============================================================================

step "停止服务..."
(cd "${INSTALL_DIR}" && docker compose down) || true

step "拉取最新镜像..."
if [[ -t 1 ]]; then
  (cd "${INSTALL_DIR}" && docker compose pull)
else
  (cd "${INSTALL_DIR}" && docker compose pull -q)
fi

# ============================================================================
# Update host binary (if host mode)
# ============================================================================

if [[ "${NBOT_MAIN_MODE}" == "host" ]]; then
  step "更新宿主机二进制文件..."

  github_wrap_url() {
    local url="$1"
    local proxy="${NBOT_GITHUB_PROXY:-}"
    if [[ -n "${proxy}" && "${proxy: -1}" != "/" ]]; then
      proxy="${proxy}/"
    fi
    if [[ -n "${proxy}" ]]; then
      echo "${proxy}${url}"
    else
      echo "${url}"
    fi
  }

  arch="$(uname -m 2>/dev/null || echo unknown)"
  case "${arch}" in
    x86_64|amd64) ;;
    *)
      warn "宿主机模式仅提供 linux-x86_64 预编译包，当前架构: ${arch}，跳过二进制更新。"
      ;;
  esac

  if [[ "${arch}" == "x86_64" || "${arch}" == "amd64" ]]; then
    asset="nbot-linux-x86_64.tar.gz"
    repo="yukmakoto/nBot"
    base="https://github.com/${repo}"
    if [[ "${NBOT_TAG}" == "latest" ]]; then
      url="${base}/releases/latest/download/${asset}"
    else
      url="${base}/releases/download/${NBOT_TAG}/${asset}"
    fi
    url="$(github_wrap_url "${url}")"

    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' RETURN

    curl -fL --retry 3 --retry-delay 2 -o "${tmp}/pkg.tar.gz" "${url}"
    mkdir -p "${tmp}/pkg"
    tar -xzf "${tmp}/pkg.tar.gz" -C "${tmp}/pkg"

    if have systemctl; then
      systemctl stop nbot >/dev/null 2>&1 || true
    fi

    install -m 0755 -D "${tmp}/pkg/backend" "${INSTALL_DIR}/backend"
    if [[ -f "${tmp}/pkg/renderd" ]]; then
      install -m 0755 -D "${tmp}/pkg/renderd" "${INSTALL_DIR}/renderd"
    fi

    rm -rf "${INSTALL_DIR}/dist" "${INSTALL_DIR}/assets"
    cp -a "${tmp}/pkg/dist" "${INSTALL_DIR}/dist"
    cp -a "${tmp}/pkg/assets" "${INSTALL_DIR}/assets"

    # Update data directory (preserve existing files)
    if [[ -d "${tmp}/pkg/data" ]]; then
      cp -a -n "${tmp}/pkg/data/." "${INSTALL_DIR}/data/" 2>/dev/null || true
    fi

    success "二进制文件已更新"
  fi
fi

# ============================================================================
# Start services
# ============================================================================

step "启动服务..."
(cd "${INSTALL_DIR}" && docker compose up -d)

if [[ "${NBOT_MAIN_MODE}" == "host" ]]; then
  if have systemctl; then
    systemctl start nbot
  fi
fi

# ============================================================================
# Health check
# ============================================================================

step "健康检查..."
NBOT_WEBUI_PORT="${NBOT_WEBUI_PORT:-32100}"
ok=""
last_code=""
for _ in $(seq 1 30); do
  code="$(curl -sS -L --max-time 2 -o /dev/null -w '%{http_code}' "http://127.0.0.1:${NBOT_WEBUI_PORT}/" 2>/dev/null || true)"
  last_code="${code}"
  # Accept any HTTP response (< 500) as "up"; only treat network errors/timeouts/5xx as failure.
  if [[ "${code}" =~ ^[0-9]{3}$ ]] && [[ "${code}" -ge 200 ]] && [[ "${code}" -lt 500 ]]; then
    ok="1"
    break
  fi
  sleep 1
done

if [[ -z "${ok}" ]]; then
  echo
  warn "WebUI 本机探测失败：127.0.0.1:${NBOT_WEBUI_PORT}（HTTP: ${last_code:-000}）"
  echo
  echo "容器状态："
  (cd "${INSTALL_DIR}" && docker compose ps -a) || true
  echo
  echo "bot 日志（最后 50 行）："
  (cd "${INSTALL_DIR}" && docker compose logs --tail 50 bot) || true
  die "启动失败，请检查日志。"
fi

# ============================================================================
# Done
# ============================================================================

echo
echo -e "${GREEN}"
cat << 'EOF'
   _   _ ____  ____    _  _____ _____ ____
  | | | |  _ \|  _ \  / \|_   _| ____|  _ \
  | | | | |_) | | | |/ _ \ | | |  _| | | | |
  | |_| |  __/| |_| / ___ \| | | |___| |_| |
   \___/|_|   |____/_/   \_\_| |_____|____/
EOF
echo -e "${NC}"

print_box "更新完成" \
  "${CHECK} 安装目录: ${WHITE}${INSTALL_DIR}${NC}" \
  "${CHECK} 部署模式: ${WHITE}${NBOT_MAIN_MODE}${NC}" \
  "${CHECK} 镜像标签: ${WHITE}${NBOT_TAG}${NC}" \
  "" \
  "${ROCKET} WebUI: ${CYAN}http://127.0.0.1:${NBOT_WEBUI_PORT}${NC}"

echo
echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GRAY}  常用命令：${NC}"
echo -e "${GRAY}    查看状态: ${WHITE}cd ${INSTALL_DIR} && docker compose ps${NC}"
echo -e "${GRAY}    查看日志: ${WHITE}cd ${INSTALL_DIR} && docker compose logs -f${NC}"
echo -e "${GRAY}    重启服务: ${WHITE}cd ${INSTALL_DIR} && docker compose restart${NC}"
echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo
