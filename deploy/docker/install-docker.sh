#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
#                              nBot Installer
#                    QQ Bot Framework - One-Click Deploy
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

# Spinner frames
SPINNER_FRAMES=('⠋' '⠙' '⠹' '⠸' '⠼' '⠴' '⠦' '⠧' '⠇' '⠏')
SPINNER_PID=""

print_banner() {
  echo -e "${CYAN}"
  cat << 'EOF'
                ____        __
      ____     / __ )____  / /_
     / __ \   / __  / __ \/ __/
    / / / /  / /_/ / /_/ / /_
   /_/ /_/  /_____/\____/\__/

   QQ Bot Framework - One-Click Deploy
EOF
  echo -e "${NC}"
  echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
  echo -e "${GRAY}  Version: ${WHITE}${NBOT_TAG:-latest}${GRAY}  |  Registry: ${WHITE}${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}${NC}"
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

# Spinner functions
start_spinner() {
  local msg="$1"
  if [[ -t 1 ]]; then
    (
      i=0
      while true; do
        printf "\r${CYAN}${SPINNER_FRAMES[$i]}${NC} %s" "$msg"
        i=$(( (i + 1) % ${#SPINNER_FRAMES[@]} ))
        sleep 0.1
      done
    ) &
    SPINNER_PID=$!
    disown
  else
    echo -e "${ARROW} $msg"
  fi
}

stop_spinner() {
  local status="$1"
  if [[ -n "${SPINNER_PID}" ]]; then
    kill "${SPINNER_PID}" 2>/dev/null || true
    wait "${SPINNER_PID}" 2>/dev/null || true
    SPINNER_PID=""
    printf "\r"
  fi
  if [[ "$status" == "success" ]]; then
    echo -e "${CHECK} ${GREEN}Done${NC}"
  elif [[ "$status" == "fail" ]]; then
    echo -e "${CROSS} ${RED}Failed${NC}"
  fi
}

# Progress bar
progress_bar() {
  local current=$1
  local total=$2
  local width=40
  local percent=$((current * 100 / total))
  local filled=$((current * width / total))
  local empty=$((width - filled))

  printf "\r${CYAN}["
  printf "%${filled}s" | tr ' ' '█'
  printf "%${empty}s" | tr ' ' '░'
  printf "]${NC} ${WHITE}%3d%%${NC}" "$percent"
}

print_section() {
  echo
  echo -e "${MAGENTA}┌─────────────────────────────────────────────────────────────────────────────┐${NC}"
  echo -e "${MAGENTA}│${NC} ${BOLD}$*${NC}"
  echo -e "${MAGENTA}└─────────────────────────────────────────────────────────────────────────────┘${NC}"
}

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
  die "请用 root 执行（示例：curl -fsSL <URL> | sudo bash）。"
fi

INSTALL_DIR="${NBOT_INSTALL_DIR:-/opt/nbot}"
COMPOSE_FILE="${INSTALL_DIR}/docker-compose.yml"
ENV_FILE="${INSTALL_DIR}/.env"
DOCKER_CONFIG_DIR="${INSTALL_DIR}/docker-config"
DOCKER_CONFIG_FILE="${DOCKER_CONFIG_DIR}/config.json"

mkdir -p "${INSTALL_DIR}" "${DOCKER_CONFIG_DIR}"
chmod 700 "${DOCKER_CONFIG_DIR}" >/dev/null 2>&1 || true

# Ensure docker CLI uses our dedicated config (keeps registry auth isolated to this install dir).
export DOCKER_CONFIG="${DOCKER_CONFIG_DIR}"

env_get() {
  local key="$1"
  local file="$2"
  [[ -f "$file" ]] || return 1
  grep -E "^[[:space:]]*${key}=" "$file" | tail -n 1 | sed -E "s/^[[:space:]]*${key}=//"
}

generate_api_token() {
  # 64 hex chars, URL-safe, no external dependencies.
  local a b
  # GNU tr treats a leading '-' in SET as an option; keep '-' after \n to avoid that.
  a="$(tr -d '\n-' </proc/sys/kernel/random/uuid)"
  b="$(tr -d '\n-' </proc/sys/kernel/random/uuid)"
  echo "${a}${b}"
}

ensure_pkg_apt() {
  apt-get update
  apt-get install -y --no-install-recommends "$@"
}

ensure_pkg_dnf() { dnf -y install "$@"; }
ensure_pkg_yum() { yum -y install "$@"; }

ensure_docker_installed() {
  if have docker; then
    return
  fi

step "Install Docker Engine + compose plugin ..."
if have apt-get; then
    ensure_pkg_apt ca-certificates curl gnupg
    . /etc/os-release || die "无法读取 /etc/os-release"
    distro_id="${ID:-}"
    codename="${VERSION_CODENAME:-}"
    [[ -n "${distro_id}" ]] || die "无法识别发行版 ID"
    [[ -n "${codename}" ]] || die "无法识别发行版 codename"

    case "${distro_id}" in
      raspbian) distro_id="debian" ;;
    esac

    docker_apt_repo="${NBOT_DOCKER_APT_REPO:-https://mirrors.aliyun.com/docker-ce/linux/${distro_id}}"

    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL "${docker_apt_repo}/gpg" | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    chmod a+r /etc/apt/keyrings/docker.gpg

    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] ${docker_apt_repo} ${codename} stable" >/etc/apt/sources.list.d/docker.list

    apt-get update
    apt-get install -y --no-install-recommends \
      docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  elif have dnf; then
    ensure_pkg_dnf dnf-plugins-core curl ca-certificates
    . /etc/os-release || die "无法读取 /etc/os-release"
    distro_id="${ID:-}"
    [[ -n "${distro_id}" ]] || die "无法识别发行版 ID"

    case "${distro_id}" in
      fedora) repo_distro="fedora" ;;
      centos|rhel|rocky|almalinux|ol) repo_distro="centos" ;;
      *) die "不支持的发行版（dnf）：${distro_id}" ;;
    esac

    docker_repo_file="${NBOT_DOCKER_RPM_REPO_FILE:-https://mirrors.aliyun.com/docker-ce/linux/${repo_distro}/docker-ce.repo}"
    dnf config-manager --add-repo "${docker_repo_file}"
    dnf -y install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  elif have yum; then
    ensure_pkg_yum yum-utils curl ca-certificates
    . /etc/os-release || die "无法读取 /etc/os-release"
    distro_id="${ID:-}"
    [[ -n "${distro_id}" ]] || die "无法识别发行版 ID"

    case "${distro_id}" in
      centos|rhel|rocky|almalinux|ol) repo_distro="centos" ;;
      *) die "不支持的发行版（yum）：${distro_id}" ;;
    esac

    docker_repo_file="${NBOT_DOCKER_RPM_REPO_FILE:-https://mirrors.aliyun.com/docker-ce/linux/${repo_distro}/docker-ce.repo}"
    yum-config-manager --add-repo "${docker_repo_file}"
    yum -y install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  else
    die "未找到包管理器（apt-get/dnf/yum），无法自动安装 Docker。"
  fi
}

ensure_docker_running() {
  docker info >/dev/null 2>&1 && return

  if have systemctl; then
    systemctl enable --now docker >/dev/null 2>&1 || true
  fi
  if have service; then
    service docker start >/dev/null 2>&1 || true
  fi

  docker info >/dev/null 2>&1 || die "Docker daemon 未运行。"
}

install_docker_compose_plugin() {
  step "Install docker compose plugin ..."

  if have apt-get; then
    ensure_pkg_apt ca-certificates curl gnupg
    . /etc/os-release || die "无法读取 /etc/os-release"
    distro_id="${ID:-}"
    codename="${VERSION_CODENAME:-}"
    [[ -n "${distro_id}" ]] || die "无法识别发行版 ID"
    [[ -n "${codename}" ]] || die "无法识别发行版 codename"

    case "${distro_id}" in
      raspbian) distro_id="debian" ;;
    esac

    docker_apt_repo="${NBOT_DOCKER_APT_REPO:-https://mirrors.aliyun.com/docker-ce/linux/${distro_id}}"

    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL "${docker_apt_repo}/gpg" | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    chmod a+r /etc/apt/keyrings/docker.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] ${docker_apt_repo} ${codename} stable" >/etc/apt/sources.list.d/docker.list

    apt-get update
    apt-get install -y --no-install-recommends \
      docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  elif have dnf; then
    ensure_pkg_dnf dnf-plugins-core curl ca-certificates
    . /etc/os-release || die "无法读取 /etc/os-release"
    distro_id="${ID:-}"
    [[ -n "${distro_id}" ]] || die "无法识别发行版 ID"

    case "${distro_id}" in
      fedora) repo_distro="fedora" ;;
      centos|rhel|rocky|almalinux|ol) repo_distro="centos" ;;
      *) die "不支持的发行版（dnf）：${distro_id}" ;;
    esac

    docker_repo_file="${NBOT_DOCKER_RPM_REPO_FILE:-https://mirrors.aliyun.com/docker-ce/linux/${repo_distro}/docker-ce.repo}"
    dnf config-manager --add-repo "${docker_repo_file}"
    dnf -y install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  elif have yum; then
    ensure_pkg_yum yum-utils curl ca-certificates
    . /etc/os-release || die "无法读取 /etc/os-release"
    distro_id="${ID:-}"
    [[ -n "${distro_id}" ]] || die "无法识别发行版 ID"

    case "${distro_id}" in
      centos|rhel|rocky|almalinux|ol) repo_distro="centos" ;;
      *) die "不支持的发行版（yum）：${distro_id}" ;;
    esac

    docker_repo_file="${NBOT_DOCKER_RPM_REPO_FILE:-https://mirrors.aliyun.com/docker-ce/linux/${repo_distro}/docker-ce.repo}"
    yum-config-manager --add-repo "${docker_repo_file}"
    yum -y install docker-ce docker-ce-cli containerd.io docker-buildx-plugin docker-compose-plugin
  else
    die "未找到包管理器（apt-get/dnf/yum），无法自动安装 docker compose。"
  fi
}

ensure_docker_compose() {
  if docker compose version >/dev/null 2>&1; then
    return
  fi

  install_docker_compose_plugin
  ensure_docker_running

  docker compose version >/dev/null 2>&1 || die "docker compose 仍不可用（请检查 docker-compose-plugin 是否安装成功）。"
}

is_port_in_use() {
  local port="$1"
  if have ss; then
    ss -lntH 2>/dev/null | grep -qE "[:.]${port}[[:space:]]" && return 0 || return 1
  fi
  (echo >/dev/tcp/127.0.0.1/"$port") >/dev/null 2>&1 && return 0 || return 1
}

rand_port() {
  echo $(( (RANDOM<<15 | RANDOM) % 28001 + 32000 ))
}

pick_free_port() {
  for _ in $(seq 1 120); do
    p="$(rand_port)"
    if ! is_port_in_use "$p"; then
      echo "$p"
      return 0
    fi
  done
  return 1
}

tty_read() {
  local prompt="$1"
  local default="${2:-}"
  local out=""

  if [[ -r /dev/tty ]]; then
    if [[ -n "${default}" ]]; then
      read -r -p "${prompt} [${default}] " out </dev/tty || true
      echo "${out:-${default}}"
    else
      read -r -p "${prompt} " out </dev/tty || true
      echo "${out}"
    fi
    return 0
  fi

  echo "${default}"
}

tty_read_secret() {
  local prompt="$1"
  local out=""

  if [[ -r /dev/tty ]]; then
    read -r -s -p "${prompt}: " out </dev/tty || true
    echo >&2
    echo "${out}"
    return 0
  fi

  echo ""
}

normalize_registry() {
  local r="$1"
  r="${r#http://}"
  r="${r#https://}"
  r="${r%/}"
  echo "$r"
}

detect_public_ipv4() {
  # Best-effort: query an external service (works even when host only has private IP behind NAT).
  is_ipv4() {
    local v="$1"
    [[ "${v}" =~ ^[0-9]+\.[0-9]+\.[0-9]+\.[0-9]+$ ]]
  }

  curl_meta() {
    # Do not use proxies for metadata endpoints.
    curl -4 -fsSL --noproxy '*' --connect-timeout 0.4 --max-time 1 "$1" 2>/dev/null || true
  }

  curl_ip() {
    curl -4 -fsSL --connect-timeout 1 --max-time 4 "$1" 2>/dev/null || true
  }

  local ip=""

  # Cloud metadata: Aliyun ECS (100.100.100.200) / AWS&others (169.254.169.254)
  # Aliyun ECS: Elastic/Public IPv4
  for path in \
    "http://100.100.100.200/latest/meta-data/eipv4" \
    "http://100.100.100.200/latest/meta-data/public-ipv4" \
    "http://100.100.100.200/latest/meta-data/public_ipv4" \
    "http://100.100.100.200/latest/meta-data/eip" \
    "http://100.100.100.200/latest/meta-data/eip-ip-address" \
    ; do
    ip="$(curl_meta "${path}")"
    ip="$(echo "${ip}" | tr -d ' \t\r\n')"
    if is_ipv4 "${ip}"; then
      echo "${ip}"
      return 0
    fi
  done

  # Tencent Cloud metadata (may be accessible by hostname as well)
  for path in \
    "http://metadata.tencentyun.com/latest/meta-data/public-ipv4" \
    "http://169.254.169.254/latest/meta-data/public-ipv4" \
    ; do
    ip="$(curl_meta "${path}")"
    ip="$(echo "${ip}" | tr -d ' \t\r\n')"
    if is_ipv4 "${ip}"; then
      echo "${ip}"
      return 0
    fi
  done

  # External services (may be blocked in some regions).
  for url in \
    "https://api.ipify.org" \
    "https://4.ipw.cn" \
    "https://ip.sb" \
    "https://ifconfig.me/ip" \
    "https://icanhazip.com" \
    "https://ipinfo.io/ip" \
    ; do
    ip="$(curl_ip "${url}")"
    ip="$(echo "${ip}" | tr -d ' \t\r\n')"
    if is_ipv4 "${ip}"; then
      echo "${ip}"
      return 0
    fi
  done

  return 1
}

detect_primary_ipv4() {
  # Best-effort LAN IP (may be private IP on cloud).
  if have ip; then
    ip -4 route get 1.1.1.1 2>/dev/null | awk '/src/ {for (i=1;i<=NF;i++) if ($i=="src") {print $(i+1); exit}}'
    return 0
  fi
  if have hostname; then
    hostname -I 2>/dev/null | awk '{print $1}'
    return 0
  fi
  return 1
}

resolve_webui_display_host() {
  local bind_host="${NBOT_WEBUI_BIND_HOST}"
  local public_host="${NBOT_WEBUI_PUBLIC_HOST:-}"

  if [[ -n "${public_host}" ]]; then
    echo "${public_host}"
    return 0
  fi

  if [[ "${bind_host}" != "0.0.0.0" && "${bind_host}" != "127.0.0.1" && "${bind_host}" != "localhost" ]]; then
    echo "${bind_host}"
    return 0
  fi

  if [[ "${bind_host}" == "0.0.0.0" ]]; then
    local ip
    ip="$(detect_public_ipv4 || true)"
    if [[ -n "${ip}" ]]; then
      echo "${ip}"
      return 0
    fi
    ip="$(detect_primary_ipv4 || true)"
    if [[ -n "${ip}" ]]; then
      echo "${ip}"
      return 0
    fi
  fi

  echo "127.0.0.1"
}

docker_login_if_needed() {
  local raw="${NBOT_DOCKER_REGISTRY:-}"
  local registry
  registry="$(normalize_registry "${raw}")"
  [[ -n "${registry}" ]] || return 0

  case "${registry}" in
    docker.io|index.docker.io|registry-1.docker.io) return 0 ;;
  esac

  # If auth already exists in our dedicated config, skip prompting.
  if [[ -f "${DOCKER_CONFIG_FILE}" ]] && grep -Fq "\"${registry}\"" "${DOCKER_CONFIG_FILE}"; then
    return 0
  fi

  local user="${NBOT_DOCKER_REGISTRY_USERNAME:-${NBOT_REGISTRY_USERNAME:-}}"
  local pass="${NBOT_DOCKER_REGISTRY_PASSWORD:-${NBOT_REGISTRY_PASSWORD:-}}"

  if [[ -z "${user}" || -z "${pass}" ]]; then
    echo
    echo "Registry 登录：${registry}"
    echo "该镜像源需要鉴权才能拉取镜像（用户名/密码不会回显）。"
    user="$(tty_read "用户名（留空取消）" "")"
    if [[ -z "${user}" ]]; then
      die "未提供用户名，已取消登录。"
    fi
    pass="$(tty_read_secret "密码（不会回显）")"
    if [[ -z "${pass}" ]]; then
      die "密码为空，已取消登录。"
    fi
  fi

  local auth
  auth="$(printf '%s' "${user}:${pass}" | base64 | tr -d '\n')"
  mkdir -p "${DOCKER_CONFIG_DIR}"
  chmod 700 "${DOCKER_CONFIG_DIR}" >/dev/null 2>&1 || true
  cat >"${DOCKER_CONFIG_FILE}" <<EOF
{"auths":{"${registry}":{"auth":"${auth}"}}}
EOF
  chmod 600 "${DOCKER_CONFIG_FILE}" >/dev/null 2>&1 || true
  pass=""
  user=""
  step "Registry 登录成功：${registry}"
}

ensure_cli_tools() {
  have curl && have tar && have base64 && return 0

  if have apt-get; then
    ensure_pkg_apt ca-certificates curl tar coreutils
  elif have dnf; then
    ensure_pkg_dnf ca-certificates curl tar coreutils
  elif have yum; then
    ensure_pkg_yum ca-certificates curl tar coreutils
  else
    die "未找到包管理器（apt-get/dnf/yum），无法自动安装 curl/tar/base64。"
  fi
}

ensure_ffmpeg() {
  have ffmpeg && have ffprobe && return 0
  step "Install ffmpeg (host) ..."
  if have apt-get; then
    ensure_pkg_apt ffmpeg
  elif have dnf; then
    ensure_pkg_dnf ffmpeg
  elif have yum; then
    ensure_pkg_yum ffmpeg
  else
    die "未找到包管理器（apt-get/dnf/yum），无法自动安装 ffmpeg。"
  fi
  have ffmpeg || die "ffmpeg 安装失败。"
}

write_compose_docker_mode() {
  step "Write ${COMPOSE_FILE}"
  local webui_port_spec
  local render_port_spec
  if [[ "${NBOT_WEBUI_BIND_HOST}" == "0.0.0.0" ]]; then
    webui_port_spec="${NBOT_WEBUI_PORT}:32100"
  else
    webui_port_spec="${NBOT_WEBUI_BIND_HOST}:${NBOT_WEBUI_PORT}:32100"
  fi

  if [[ "${NBOT_RENDER_BIND_HOST}" == "0.0.0.0" ]]; then
    render_port_spec="${NBOT_RENDER_PORT}:8080"
  else
    render_port_spec="${NBOT_RENDER_BIND_HOST}:${NBOT_RENDER_PORT}:8080"
  fi

  cat >"${COMPOSE_FILE}" <<'YAML'
services:
  bot:
    image: ${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}/${NBOT_DOCKERHUB_NAMESPACE:-yukmakoto}/nbot-bot:${NBOT_TAG:-latest}
    restart: unless-stopped
    ports:
      - "__NBOT_WEBUI_PORT_SPEC__"
    depends_on:
      - wkhtmltoimage
    environment:
      - RUST_LOG=info
      - WKHTMLTOIMAGE_URL=http://wkhtmltoimage:8080
      - NBOT_TWEMOJI_BASE_URL=${NBOT_TWEMOJI_BASE_URL:-https://cdn.staticfile.org/twemoji/14.0.2/svg/}
      - NBOT_DOCKERHUB_NAMESPACE=${NBOT_DOCKERHUB_NAMESPACE:-yukmakoto}
      - NBOT_DOCKER_REGISTRY=${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}
      - NBOT_TAG=${NBOT_TAG:-latest}
      - NBOT_NAPCAT_IMAGE=${NBOT_NAPCAT_IMAGE:-docker.nailed.dev/yukmakoto/napcat-docker:latest}
      - NBOT_API_TOKEN=${NBOT_API_TOKEN}
      - NBOT_DOCKER_MODE=true
      - DOCKER_CONFIG=/docker-config
      - PROJECT_ROOT=/compose
      - COMPOSE_PROJECT_NAME=${COMPOSE_PROJECT_NAME:-nbot}
    volumes:
      - nbot_data:/app/data
      - /var/run/docker.sock:/var/run/docker.sock
      - ./:/compose:ro
      - ./docker-config:/docker-config:ro
    networks:
      - nbot_default

  wkhtmltoimage:
    image: ${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}/${NBOT_DOCKERHUB_NAMESPACE:-yukmakoto}/nbot-render:${NBOT_TAG:-latest}
    restart: unless-stopped
    ports:
      - "__NBOT_RENDER_PORT_SPEC__"
    environment:
      - RUST_LOG=info
      - PORT=8080
    networks:
      - nbot_default
    labels:
      - "nbot.infra=true"
      - "nbot.infra.name=wkhtmltoimage"
      - "nbot.infra.description=HTML 转图片服务"

volumes:
  nbot_data:

networks:
  nbot_default:
    name: nbot_default
    driver: bridge
YAML

  sed -i \
    -e "s|__NBOT_WEBUI_PORT_SPEC__|${webui_port_spec}|g" \
    -e "s|__NBOT_RENDER_PORT_SPEC__|${render_port_spec}|g" \
    "${COMPOSE_FILE}"
}

write_compose_tools_only() {
  step "Write ${COMPOSE_FILE}"
  local render_port_spec
  if [[ "${NBOT_RENDER_BIND_HOST}" == "0.0.0.0" ]]; then
    render_port_spec="${NBOT_RENDER_PORT}:8080"
  else
    render_port_spec="${NBOT_RENDER_BIND_HOST}:${NBOT_RENDER_PORT}:8080"
  fi

  cat >"${COMPOSE_FILE}" <<'YAML'
services:
  wkhtmltoimage:
    image: ${NBOT_DOCKER_REGISTRY:-docker.nailed.dev}/${NBOT_DOCKERHUB_NAMESPACE:-yukmakoto}/nbot-render:${NBOT_TAG:-latest}
    restart: unless-stopped
    ports:
      - "__NBOT_RENDER_PORT_SPEC__"
    environment:
      - RUST_LOG=info
      - PORT=8080
    networks:
      - nbot_default
    labels:
      - "nbot.infra=true"
      - "nbot.infra.name=wkhtmltoimage"
      - "nbot.infra.description=HTML 转图片服务"

networks:
  nbot_default:
    name: nbot_default
    driver: bridge
YAML

  sed -i -e "s|__NBOT_RENDER_PORT_SPEC__|${render_port_spec}|g" "${COMPOSE_FILE}"
}

compose_down() {
  (cd "${INSTALL_DIR}" && docker compose down --remove-orphans) >/dev/null 2>&1 || true
}

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

install_host_package() {
  local arch
  arch="$(uname -m 2>/dev/null || echo unknown)"
  case "${arch}" in
    x86_64|amd64) ;;
    *)
      die "宿主机模式仅提供 linux-x86_64 预编译包，当前架构: ${arch}（请改用 Docker 模式）。"
      ;;
  esac

  ensure_cli_tools
  ensure_ffmpeg

  local asset="nbot-linux-x86_64.tar.gz"
  local repo="yukmakoto/nBot"
  local base="https://github.com/${repo}"
  local url
  if [[ "${NBOT_TAG}" == "latest" ]]; then
    url="${base}/releases/latest/download/${asset}"
  else
    url="${base}/releases/download/${NBOT_TAG}/${asset}"
  fi
  url="$(github_wrap_url "${url}")"

  step "Download release package (${asset}, tag=${NBOT_TAG}) ..."
  local tmp
  tmp="$(mktemp -d)"
  trap 'rm -rf "${tmp}"' RETURN

  curl -fL --retry 3 --retry-delay 2 -o "${tmp}/pkg.tar.gz" "${url}"
  mkdir -p "${tmp}/pkg"
  tar -xzf "${tmp}/pkg.tar.gz" -C "${tmp}/pkg"

  if ! have systemctl; then
    die "宿主机模式需要 systemd（systemctl）。请改用 Docker 模式或手动启动。"
  fi

  systemctl stop nbot >/dev/null 2>&1 || true

  install -m 0755 -D "${tmp}/pkg/backend" "${INSTALL_DIR}/backend"
  if [[ -f "${tmp}/pkg/renderd" ]]; then
    install -m 0755 -D "${tmp}/pkg/renderd" "${INSTALL_DIR}/renderd"
  fi

  rm -rf "${INSTALL_DIR}/dist" "${INSTALL_DIR}/assets"
  cp -a "${tmp}/pkg/dist" "${INSTALL_DIR}/dist"
  cp -a "${tmp}/pkg/assets" "${INSTALL_DIR}/assets"

  mkdir -p "${INSTALL_DIR}/data"
  if [[ -d "${tmp}/pkg/data" ]]; then
    cp -a -n "${tmp}/pkg/data/." "${INSTALL_DIR}/data/" || true
  fi
  mkdir -p "${INSTALL_DIR}/data/state"

  if [[ -f "${tmp}/pkg/.env.example" && ! -f "${INSTALL_DIR}/.env.example" ]]; then
    cp -a "${tmp}/pkg/.env.example" "${INSTALL_DIR}/.env.example"
  fi
  if [[ -f "${tmp}/pkg/LICENSE" ]]; then
    cp -a "${tmp}/pkg/LICENSE" "${INSTALL_DIR}/LICENSE"
  fi
}

install_systemd_unit() {
  local unit="/etc/systemd/system/nbot.service"
  step "Write ${unit}"
  cat >"${unit}" <<EOF
[Unit]
Description=nBot Backend
After=network-online.target docker.service
Wants=network-online.target docker.service

[Service]
Type=simple
WorkingDirectory=${INSTALL_DIR}
EnvironmentFile=${ENV_FILE}
Environment=DOCKER_CONFIG=${DOCKER_CONFIG_DIR}
ExecStart=${INSTALL_DIR}/backend
Restart=always
RestartSec=2

[Install]
WantedBy=multi-user.target
EOF

  systemctl daemon-reload
  systemctl enable --now nbot
}

existing_namespace="$(env_get NBOT_DOCKERHUB_NAMESPACE "${ENV_FILE}" || true)"
existing_tag="$(env_get NBOT_TAG "${ENV_FILE}" || true)"
existing_registry="$(env_get NBOT_DOCKER_REGISTRY "${ENV_FILE}" || true)"
existing_twemoji="$(env_get NBOT_TWEMOJI_BASE_URL "${ENV_FILE}" || true)"
existing_napcat="$(env_get NBOT_NAPCAT_IMAGE "${ENV_FILE}" || true)"
existing_webui_bind_host="$(env_get NBOT_WEBUI_BIND_HOST "${ENV_FILE}" || true)"
existing_webui_host_legacy="$(env_get NBOT_WEBUI_HOST "${ENV_FILE}" || true)"
existing_webui_public_host="$(env_get NBOT_WEBUI_PUBLIC_HOST "${ENV_FILE}" || true)"
existing_webui_port="$(env_get NBOT_WEBUI_PORT "${ENV_FILE}" || true)"
existing_render_bind_host="$(env_get NBOT_RENDER_BIND_HOST "${ENV_FILE}" || true)"
existing_render_host_legacy="$(env_get NBOT_RENDER_HOST "${ENV_FILE}" || true)"
existing_render_port="$(env_get NBOT_RENDER_PORT "${ENV_FILE}" || true)"
existing_project="$(env_get COMPOSE_PROJECT_NAME "${ENV_FILE}" || true)"
existing_main_mode="$(env_get NBOT_MAIN_MODE "${ENV_FILE}" || true)"
existing_api_token="$(env_get NBOT_API_TOKEN "${ENV_FILE}" || true)"

NBOT_DOCKERHUB_NAMESPACE="${NBOT_DOCKERHUB_NAMESPACE:-${existing_namespace:-yukmakoto}}"
NBOT_TAG="${NBOT_TAG:-${existing_tag:-latest}}"
NBOT_DOCKER_REGISTRY="${NBOT_DOCKER_REGISTRY:-${existing_registry:-docker.nailed.dev}}"
NBOT_TWEMOJI_BASE_URL="${NBOT_TWEMOJI_BASE_URL:-${existing_twemoji:-https://cdn.staticfile.org/twemoji/14.0.2/svg/}}"
NBOT_NAPCAT_IMAGE="${NBOT_NAPCAT_IMAGE:-${existing_napcat:-docker.nailed.dev/yukmakoto/napcat-docker:latest}}"
NBOT_WEBUI_BIND_HOST="${NBOT_WEBUI_BIND_HOST:-${existing_webui_bind_host:-${existing_webui_host_legacy:-0.0.0.0}}}"
NBOT_WEBUI_PUBLIC_HOST="${NBOT_WEBUI_PUBLIC_HOST:-${existing_webui_public_host:-}}"
NBOT_WEBUI_PORT="${NBOT_WEBUI_PORT:-${existing_webui_port:-}}"
NBOT_RENDER_BIND_HOST="${NBOT_RENDER_BIND_HOST:-${existing_render_bind_host:-${existing_render_host_legacy:-127.0.0.1}}}"
NBOT_RENDER_PORT="${NBOT_RENDER_PORT:-${existing_render_port:-}}"
COMPOSE_PROJECT_NAME="${COMPOSE_PROJECT_NAME:-${existing_project:-nbot}}"
NBOT_API_TOKEN="${NBOT_API_TOKEN:-${existing_api_token:-}}"

if [[ -z "${NBOT_MAIN_MODE:-}" ]]; then
  if [[ -n "${existing_main_mode}" ]]; then
    NBOT_MAIN_MODE="${existing_main_mode}"
  fi
fi

show_menu() {
  local title="$1"
  shift
  local options=("$@")
  local selected=0
  local key=""

  # If not a TTY, return default (first option)
  if [[ ! -t 0 ]]; then
    echo "1"
    return
  fi

  # Hide cursor
  tput civis 2>/dev/null || true

  while true; do
    # Clear menu area and redraw
    echo -e "\n${CYAN}${BOLD}${title}${NC}\n"

    for i in "${!options[@]}"; do
      if [[ $i -eq $selected ]]; then
        echo -e "  ${GREEN}▸ ${BOLD}${options[$i]}${NC}"
      else
        echo -e "  ${GRAY}  ${options[$i]}${NC}"
      fi
    done

    echo -e "\n${DIM}使用 ↑/↓ 或 j/k 选择，Enter 确认${NC}"

    # Read single key
    IFS= read -rsn1 key </dev/tty 2>/dev/null || { echo "$((selected + 1))"; tput cnorm 2>/dev/null || true; return; }

    case "$key" in
      $'\x1b')  # Escape sequence
        read -rsn2 -t 0.1 key </dev/tty 2>/dev/null || true
        case "$key" in
          '[A'|'OA') ((selected > 0)) && ((selected--)) ;;  # Up
          '[B'|'OB') ((selected < ${#options[@]} - 1)) && ((selected++)) ;;  # Down
        esac
        ;;
      'k'|'K') ((selected > 0)) && ((selected--)) ;;
      'j'|'J') ((selected < ${#options[@]} - 1)) && ((selected++)) ;;
      ''|$'\n') break ;;  # Enter
      [1-9])
        if [[ "$key" -le "${#options[@]}" ]]; then
          selected=$((key - 1))
          break
        fi
        ;;
    esac

    # Move cursor up to redraw menu
    for _ in "${options[@]}"; do
      tput cuu1 2>/dev/null || printf '\033[1A'
    done
    tput cuu1 2>/dev/null || printf '\033[1A'
    tput cuu1 2>/dev/null || printf '\033[1A'
    tput cuu1 2>/dev/null || printf '\033[1A'
  done

  # Show cursor
  tput cnorm 2>/dev/null || true

  echo "$((selected + 1))"
}

if [[ -z "${NBOT_MAIN_MODE:-}" ]]; then
  if [[ -n "${existing_main_mode}" ]]; then
    NBOT_MAIN_MODE="${existing_main_mode}"
  fi
fi

if [[ -z "${NBOT_MAIN_MODE:-}" ]]; then
  print_section "选择部署模式"

  if [[ -t 0 ]]; then
    echo -e "\n${CYAN}${BOLD}选择主程序运行方式：${NC}\n"
    echo -e "  ${GREEN}▸ ${BOLD}1) Docker 模式（推荐）${NC}"
    echo -e "     ${GRAY}主程序 + 渲染均运行在 Docker 容器中${NC}"
    echo -e "     ${GRAY}优点：隔离性好、易于管理、跨平台一致${NC}"
    echo
    echo -e "  ${WHITE}  2) 宿主机模式${NC}"
    echo -e "     ${GRAY}主程序运行在宿主机（二进制），渲染使用 Docker${NC}"
    echo -e "     ${GRAY}优点：性能略高、调试方便${NC}"
    echo

    choice="$(tty_read "请选择 [1/2]" "1")"
  else
    choice="1"
  fi

  case "${choice}" in
    1|docker) NBOT_MAIN_MODE="docker" ;;
    2|host) NBOT_MAIN_MODE="host" ;;
    *) die "无效选择: ${choice}（请输入 1/2 或 docker/host）" ;;
  esac

  success "已选择: ${NBOT_MAIN_MODE} 模式"
fi

case "${NBOT_MAIN_MODE}" in
  docker|host) ;;
  *) die "NBOT_MAIN_MODE 无效: ${NBOT_MAIN_MODE}（仅支持 docker/host）" ;;
esac

if [[ -z "${NBOT_WEBUI_BIND_HOST}" ]]; then
  NBOT_WEBUI_BIND_HOST="0.0.0.0"
fi

#
# 默认公网开放（0.0.0.0）。
# 如需仅本机访问，请在执行前设置 NBOT_WEBUI_BIND_HOST=127.0.0.1，或编辑安装目录的 .env 后重启。
#

if [[ -z "${NBOT_WEBUI_PORT}" ]]; then
  NBOT_WEBUI_PORT="$(pick_free_port)" || die "无法挑选 WebUI 空闲端口。"
fi

if [[ -z "${NBOT_RENDER_PORT}" ]]; then
  for _ in $(seq 1 40); do
    p="$(pick_free_port)" || break
    if [[ "$p" != "${NBOT_WEBUI_PORT}" ]]; then
      NBOT_RENDER_PORT="$p"
      break
    fi
  done
  [[ -n "${NBOT_RENDER_PORT}" ]] || die "无法挑选渲染服务空闲端口。"
fi

if [[ -z "${NBOT_API_TOKEN}" ]]; then
  NBOT_API_TOKEN="$(generate_api_token)"
fi

NBOT_PORT="${NBOT_PORT:-${NBOT_WEBUI_PORT}}"
NBOT_BIND="${NBOT_BIND:-${NBOT_WEBUI_BIND_HOST}:${NBOT_WEBUI_PORT}}"
WKHTMLTOIMAGE_URL="${WKHTMLTOIMAGE_URL:-http://${NBOT_RENDER_BIND_HOST}:${NBOT_RENDER_PORT}}"

step "Write ${ENV_FILE}"
cat >"${ENV_FILE}" <<EOF
NBOT_MAIN_MODE=${NBOT_MAIN_MODE}
NBOT_DOCKERHUB_NAMESPACE=${NBOT_DOCKERHUB_NAMESPACE}
NBOT_TAG=${NBOT_TAG}
NBOT_DOCKER_REGISTRY=${NBOT_DOCKER_REGISTRY}
NBOT_TWEMOJI_BASE_URL=${NBOT_TWEMOJI_BASE_URL}
NBOT_NAPCAT_IMAGE=${NBOT_NAPCAT_IMAGE}
NBOT_WEBUI_BIND_HOST=${NBOT_WEBUI_BIND_HOST}
NBOT_WEBUI_PUBLIC_HOST=${NBOT_WEBUI_PUBLIC_HOST}
NBOT_WEBUI_PORT=${NBOT_WEBUI_PORT}
NBOT_PORT=${NBOT_PORT}
NBOT_BIND=${NBOT_BIND}
NBOT_RENDER_BIND_HOST=${NBOT_RENDER_BIND_HOST}
NBOT_RENDER_PORT=${NBOT_RENDER_PORT}
WKHTMLTOIMAGE_URL=${WKHTMLTOIMAGE_URL}
NBOT_API_TOKEN=${NBOT_API_TOKEN}
COMPOSE_PROJECT_NAME=${COMPOSE_PROJECT_NAME}
EOF

ensure_docker_installed
ensure_docker_running
ensure_docker_compose
ensure_cli_tools
ensure_ffmpeg

compose_down

if [[ "${NBOT_MAIN_MODE}" == "docker" ]]; then
  if have systemctl; then
    systemctl disable --now nbot >/dev/null 2>&1 || true
  fi
  write_compose_docker_mode
else
  install_host_package
  write_compose_tools_only
fi

step "Pull images (no build) ..."
pull_out=""
pull_code=0
pull_log="$(mktemp)"
start_spinner "Pulling images ..."
set +e
(cd "${INSTALL_DIR}" && docker compose pull -q) >"${pull_log}" 2>&1
pull_code=$?
set -e
stop_spinner "$([[ "${pull_code}" -eq 0 ]] && echo success || echo fail)"

if [[ "${pull_code}" -ne 0 ]]; then
  pull_out="$(cat "${pull_log}")"
  echo "${pull_out}"
  if echo "${pull_out}" | grep -qiE "unauthorized|authentication required|denied|pull access denied"; then
    echo
    echo "镜像拉取需要登录镜像仓库。"
    docker_login_if_needed
    start_spinner "Pulling images (after login) ..."
    set +e
    (cd "${INSTALL_DIR}" && docker compose pull -q) >"${pull_log}" 2>&1
    pull_code=$?
    set -e
    stop_spinner "$([[ "${pull_code}" -eq 0 ]] && echo success || echo fail)"
    if [[ "${pull_code}" -ne 0 ]]; then
      echo "$(cat "${pull_log}")"
      die "镜像拉取失败（鉴权后仍失败）。"
    fi
  else
    die "镜像拉取失败（非鉴权错误）。"
  fi
fi
rm -f "${pull_log}" >/dev/null 2>&1 || true

step "Pull NapCat image: ${NBOT_NAPCAT_IMAGE} ..."
napcat_pull_code=0
napcat_pull_log="$(mktemp)"
start_spinner "Pulling NapCat image ..."
set +e
docker pull -q "${NBOT_NAPCAT_IMAGE}" >"${napcat_pull_log}" 2>&1
napcat_pull_code=$?
set -e
stop_spinner "$([[ "${napcat_pull_code}" -eq 0 ]] && echo success || echo fail)"
if [[ "${napcat_pull_code}" -eq 0 ]]; then
  success "NapCat 镜像拉取成功"
else
  if [[ -s "${napcat_pull_log}" ]]; then
    echo
    echo "NapCat 拉取输出（截断）："
    tail -n 80 "${napcat_pull_log}" 2>/dev/null || cat "${napcat_pull_log}" || true
  fi
  warn "NapCat 镜像拉取失败，创建机器人实例时会自动重试"
fi
rm -f "${napcat_pull_log}" >/dev/null 2>&1 || true

step "Start services ..."
(cd "${INSTALL_DIR}" && docker compose up -d 2>&1)

if [[ "${NBOT_MAIN_MODE}" == "host" ]]; then
  install_systemd_unit
fi

step "Health check (WebUI) ..."
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
  echo "WebUI 本机探测失败：127.0.0.1:${NBOT_WEBUI_PORT}（HTTP: ${last_code:-000}）"
  echo
  echo "容器状态："
  (cd "${INSTALL_DIR}" && docker compose ps -a) || true
  echo
  echo "bot 日志（最后 200 行）："
  (cd "${INSTALL_DIR}" && docker compose logs --tail 200 bot) || true
  echo
  echo "wkhtmltoimage 日志（最后 200 行）："
  (cd "${INSTALL_DIR}" && docker compose logs --tail 200 wkhtmltoimage) || true
  echo
  die "启动失败：请根据上方日志排查（常见原因：镜像未正确拉取/容器启动即退出/端口被占用/安全组未放行仅影响公网但不影响本机）。"
fi

echo
echo -e "${GREEN}"
cat << 'EOF'
   _____ _    _  _____ _____ ______  _____ _____
  / ____| |  | |/ ____/ ____|  ____|/ ____/ ____|
 | (___ | |  | | |   | |    | |__  | (___| (___
  \___ \| |  | | |   | |    |  __|  \___ \\___ \
  ____) | |__| | |___| |____| |____ ____) |___) |
 |_____/ \____/ \_____\_____|______|_____/_____/
EOF
echo -e "${NC}"

display_host="$(resolve_webui_display_host)"
if [[ "${display_host}" == *:* && "${display_host}" != \[*\] ]]; then
  webui_url="http://[${display_host}]:${NBOT_WEBUI_PORT}"
else
  webui_url="http://${display_host}:${NBOT_WEBUI_PORT}"
fi

print_box "安装完成" \
  "${CHECK} 安装目录: ${WHITE}${INSTALL_DIR}${NC}" \
  "${CHECK} 部署模式: ${WHITE}${NBOT_MAIN_MODE}${NC}" \
  "" \
  "${ROCKET} WebUI 地址: ${CYAN}${webui_url}${NC}" \
  "${INFO} WebUI 绑定: ${GRAY}${NBOT_WEBUI_BIND_HOST}:${NBOT_WEBUI_PORT}${NC}" \
  "${INFO} 渲染服务: ${GRAY}http://${NBOT_RENDER_BIND_HOST}:${NBOT_RENDER_PORT}/health${NC}"

echo
echo -e "${YELLOW}╭─────────────────────────────────────────────────────────────────────────────╮${NC}"
echo -e "${YELLOW}│${NC}                          ${BOLD}${YELLOW}nBot 认证信息${NC}                                   ${YELLOW}│${NC}"
echo -e "${YELLOW}├─────────────────────────────────────────────────────────────────────────────┤${NC}"
echo -e "${YELLOW}│${NC}  API Token: ${WHITE}${NBOT_API_TOKEN}${NC}"
echo -e "${YELLOW}╰─────────────────────────────────────────────────────────────────────────────╯${NC}"

if [[ "${NBOT_WEBUI_BIND_HOST}" == "0.0.0.0" ]]; then
  echo
  warn "已绑定到 0.0.0.0（允许公网/局域网访问）"
  info "请确保服务器防火墙/安全组放行端口：${NBOT_WEBUI_PORT}"
fi

echo
echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${GRAY}  常用命令：${NC}"
echo -e "${GRAY}    查看状态: ${WHITE}cd ${INSTALL_DIR} && docker compose ps${NC}"
echo -e "${GRAY}    查看日志: ${WHITE}cd ${INSTALL_DIR} && docker compose logs -f${NC}"
echo -e "${GRAY}    重启服务: ${WHITE}cd ${INSTALL_DIR} && docker compose restart${NC}"
echo -e "${GRAY}    停止服务: ${WHITE}cd ${INSTALL_DIR} && docker compose down${NC}"
echo -e "${GRAY}    诊断工具: ${WHITE}bash ${INSTALL_DIR}/diagnose.sh${NC}"
echo -e "${DIM}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo
