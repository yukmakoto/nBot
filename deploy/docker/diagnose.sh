#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
#                           nBot Diagnostic Tool
#                    QQ Bot Framework - System Diagnostics
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

NBOT_DIR="${NBOT_DIR:-/opt/nbot}"
COMPOSE_FILE="${NBOT_DIR}/docker-compose.yml"

print_banner() {
  echo -e "${CYAN}"
  cat << 'EOF'
                ____        __
      ____     / __ )____  / /_
     / __ \   / __  / __ \/ __/
    / / / /  / /_/ / /_/ / /_
   /_/ /_/  /_____/\____/\__/

   Diagnostic Tool
EOF
  echo -e "${NC}"
}

section() {
  echo
  echo -e "${MAGENTA}┌─────────────────────────────────────────────────────────────────────────────┐${NC}"
  echo -e "${MAGENTA}│${NC} ${BOLD}$*${NC}"
  echo -e "${MAGENTA}└─────────────────────────────────────────────────────────────────────────────┘${NC}"
}

info_line() {
  echo -e "${ARROW} ${WHITE}$1:${NC} ${GRAY}$2${NC}"
}

ok_line() {
  echo -e "${CHECK} ${GREEN}$1${NC}"
}

warn_line() {
  echo -e "${WARN} ${YELLOW}$1${NC}"
}

error_line() {
  echo -e "${CROSS} ${RED}$1${NC}"
}

redact() {
  # Best-effort redaction for sensitive values that may appear in logs.
  sed -E \
    -e 's/(WebUI Token:)[[:space:]]*[^[:space:]]+/\1 [REDACTED]/g' \
    -e 's/(WebUi Token:)[[:space:]]*[^[:space:]]+/\1 [REDACTED]/g' \
    -e 's/(Authorization: Bearer)[[:space:]]+[^[:space:]]+/\1 [REDACTED]/g' \
    -e 's/(Bearer)[[:space:]]+[^[:space:]]+/\1 [REDACTED]/g'
}

# Clear screen and show banner
clear 2>/dev/null || true
print_banner

section "nBot Diagnose ($(date -Is))"
info_line "Host" "$(hostname)"
info_line "User" "$(id -un) (uid=$(id -u))"

if [[ -f /etc/os-release ]]; then
  section "Operating System"
  . /etc/os-release
  info_line "Name" "${PRETTY_NAME:-${NAME:-Unknown}}"
  info_line "ID" "${ID:-Unknown}"
  info_line "Version" "${VERSION_ID:-Unknown}"
fi

section "Kernel"
info_line "Kernel" "$(uname -r)"
info_line "Architecture" "$(uname -m)"

section "Docker"
if command -v docker >/dev/null; then
  ok_line "Docker installed"
  docker_version=$(docker version --format '{{.Server.Version}}' 2>/dev/null || echo "Unknown")
  info_line "Docker Version" "$docker_version"
  echo
  docker compose version 2>/dev/null || docker-compose version 2>/dev/null || warn_line "docker compose not found"
else
  error_line "Docker not installed"
fi

section "nBot Compose"
info_line "NBOT_DIR" "${NBOT_DIR}"
if [[ -f "${COMPOSE_FILE}" ]]; then
  ok_line "Compose file found: ${COMPOSE_FILE}"
  echo -e "${DIM}"
  docker compose -f "${COMPOSE_FILE}" ps 2>/dev/null || true
  echo -e "${NC}"
else
  error_line "Missing: ${COMPOSE_FILE}"
fi

section "Containers (all)"
echo -e "${DIM}"
docker ps -a --format 'table {{.Names}}\t{{.Image}}\t{{.Status}}\t{{.Ports}}'
echo -e "${NC}"

BOT_CONTAINER="$(docker ps -a --format '{{.Names}}' | awk '/^nbot-bot-/{print; exit}')"
RENDER_CONTAINER="$(docker ps -a --format '{{.Names}}' | awk '/^nbot-wkhtmltoimage-/{print; exit}')"

if [[ -n "${BOT_CONTAINER}" ]]; then
  section "Bot Container (${BOT_CONTAINER})"
  status_info=$(docker inspect -f 'Status={{.State.Status}} RestartCount={{.RestartCount}} StartedAt={{.State.StartedAt}}' "${BOT_CONTAINER}")
  info_line "Status" "$status_info"

  PORT_LINES="$(docker port "${BOT_CONTAINER}" 32100/tcp 2>/dev/null || true)"
  info_line "Port mapping (32100/tcp)" "${PORT_LINES:-<none>}"

  WEBUI_PORT="$(echo "${PORT_LINES}" | awk -F: 'NR==1{print $NF}')"
  if [[ -n "${WEBUI_PORT}" ]]; then
    echo -e "${ARROW} ${WHITE}Local curl test:${NC}"
    if curl -sS -o /dev/null -w "  HTTP %{http_code} in %{time_total}s\n" "http://127.0.0.1:${WEBUI_PORT}/" 2>/dev/null; then
      ok_line "WebUI responding"
    else
      warn_line "WebUI not responding"
    fi

    echo -e "${ARROW} ${WHITE}Listening socket:${NC}"
    (ss -lntp 2>/dev/null || netstat -lntp 2>/dev/null || true) | grep ":${WEBUI_PORT} " || warn_line "Socket not found"
  fi

  echo -e "\n${ARROW} ${WHITE}Last 120 log lines:${NC}"
  echo -e "${DIM}"
  docker logs "${BOT_CONTAINER}" --tail 120 2>/dev/null | redact || true
  echo -e "${NC}"
fi

if [[ -n "${RENDER_CONTAINER}" ]]; then
  section "Renderer Container (${RENDER_CONTAINER})"
  status_info=$(docker inspect -f 'Status={{.State.Status}} RestartCount={{.RestartCount}} StartedAt={{.State.StartedAt}}' "${RENDER_CONTAINER}")
  info_line "Status" "$status_info"

  echo -e "${ARROW} ${WHITE}Port mapping (8080/tcp):${NC}"
  docker port "${RENDER_CONTAINER}" 8080/tcp 2>/dev/null || warn_line "No port mapping"

  echo -e "\n${ARROW} ${WHITE}Last 80 log lines:${NC}"
  echo -e "${DIM}"
  docker logs "${RENDER_CONTAINER}" --tail 80 2>/dev/null | redact || true
  echo -e "${NC}"
fi

section "Firewall (quick)"
if command -v ufw >/dev/null 2>&1; then
  info_line "UFW" "detected"
  ufw status verbose 2>/dev/null || true
fi
if command -v firewall-cmd >/dev/null 2>&1; then
  info_line "firewalld" "detected"
  firewall-cmd --state 2>/dev/null || true
  firewall-cmd --list-all 2>/dev/null || true
fi
if command -v iptables >/dev/null 2>&1; then
  info_line "iptables" "checking INPUT rules"
  echo -e "${DIM}"
  iptables -S INPUT 2>/dev/null | sed -n '1,200p' || true
  echo -e "${NC}"
fi
if command -v nft >/dev/null 2>&1; then
  info_line "nftables" "checking ruleset"
  echo -e "${DIM}"
  nft list ruleset 2>/dev/null | sed -n '1,200p' || true
  echo -e "${NC}"
fi

echo
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo -e "${CHECK} ${GREEN}Diagnostic complete${NC}"
echo -e "${GREEN}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${NC}"
echo
