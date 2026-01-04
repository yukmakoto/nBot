#!/usr/bin/env bash
set -euo pipefail

REPO="${1:-yukmakoto/nBot}"

have() { command -v "$1" >/dev/null 2>&1; }
die() { echo "ERROR: $*" >&2; exit 1; }
step() { echo ">>> $*"; }

have gh || die "GitHub CLI (gh) not found. Install: https://github.com/cli/cli#installation"

step "Verify GitHub auth ..."
gh auth status >/dev/null 2>&1 || die "Not logged into GitHub CLI. Run: gh auth login"

echo
echo "Docker Hub 账号/Token 获取方式："
echo "1) 注册: https://hub.docker.com/signup"
echo "2) 生成 Token: Docker Hub -> Account Settings -> Security -> New Access Token (Read & Write)"
echo

read -r -p "Docker Hub Username: " DOCKERHUB_USERNAME
[[ -n "${DOCKERHUB_USERNAME}" ]] || die "Username is empty."

read -r -s -p "Docker Hub Access Token (不会回显): " DOCKERHUB_TOKEN
echo
[[ -n "${DOCKERHUB_TOKEN}" ]] || die "Token is empty."

step "Set GitHub Actions secrets for ${REPO} ..."
printf "%s" "${DOCKERHUB_USERNAME}" | gh secret set DOCKERHUB_USERNAME --repo "${REPO}" --body-file=-
printf "%s" "${DOCKERHUB_TOKEN}" | gh secret set DOCKERHUB_TOKEN --repo "${REPO}" --body-file=-

step "Done."
echo "Secrets set: DOCKERHUB_USERNAME, DOCKERHUB_TOKEN"
