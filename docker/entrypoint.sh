#!/usr/bin/env bash
set -euo pipefail

ROOT="/app"
SEED_DIR="${ROOT}/data.seed"

DATA_DIR="${NBOT_DATA_DIR:-data}"
if [[ "${DATA_DIR}" != /* ]]; then
  DATA_DIR="${ROOT}/${DATA_DIR}"
fi

mkdir -p "${DATA_DIR}"

if [[ -d "${SEED_DIR}" ]]; then
  cp -a -n "${SEED_DIR}/." "${DATA_DIR}/"
fi

mkdir -p "${DATA_DIR}/state"

exec "$@"

