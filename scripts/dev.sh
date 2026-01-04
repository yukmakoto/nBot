#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing dependency: $1" >&2
    exit 1
  fi
}

require_cmd cargo
require_cmd npm
require_cmd docker

version_ge() {
  local IFS=.
  local i
  local ver_a=($1)
  local ver_b=($2)

  for ((i=${#ver_a[@]}; i<${#ver_b[@]}; i++)); do ver_a[i]=0; done
  for ((i=0; i<${#ver_a[@]}; i++)); do
    if [[ -z "${ver_b[i]:-}" ]]; then ver_b[i]=0; fi
    if ((10#${ver_a[i]} > 10#${ver_b[i]})); then return 0; fi
    if ((10#${ver_a[i]} < 10#${ver_b[i]})); then return 1; fi
  done
  return 0
}

required_rust="1.88.0"
current_rust="$(rustc --version | awk '{print $2}')"
if ! version_ge "$current_rust" "$required_rust"; then
  echo "Rust toolchain too old ($current_rust). Require rustc >= $required_rust" >&2
  exit 1
fi

echo ">>> Tool: start wkhtmltoimage renderer"
cd "$ROOT_DIR"
export NBOT_RENDER_PORT="${NBOT_RENDER_PORT:-32180}"
docker compose up -d --build wkhtmltoimage

echo ">>> Frontend: install deps"
cd "$ROOT_DIR/webui"
npm install

echo ">>> WebUI (React): build dist/"
npm run build

echo ">>> Start backend"
export NBOT_PORT="${NBOT_PORT:-32100}"
echo "WebUI: http://localhost:${NBOT_PORT}"
export WKHTMLTOIMAGE_URL="${WKHTMLTOIMAGE_URL:-http://localhost:${NBOT_RENDER_PORT}}"
cargo run -p backend --bin backend
