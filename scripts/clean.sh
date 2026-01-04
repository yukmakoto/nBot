#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

rm -rf target dist frontend/node_modules frontend/dist webui/node_modules webui/public/nbot_logo.png backend/dist frontend/public/tailwind.css
echo "Done."
