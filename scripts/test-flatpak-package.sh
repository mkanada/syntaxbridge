#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="${FLATPAK_APP_ID:-dev.syntax_bridge.SyntaxBridge}"
MANIFEST="${ROOT_DIR}/build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json"
BUILD_DIR="${FLATPAK_BUILD_DIR:-/tmp/syntax-bridge-flatpak-build}"
STATE_DIR="${FLATPAK_STATE_DIR:-/tmp/syntax-bridge-flatpak-state}"

flatpak info org.flatpak.Builder >/dev/null

(
  cd "${ROOT_DIR}/client/flutter"
  flutter build linux --release
)

flatpak run \
  --filesystem="${ROOT_DIR}:rw" \
  --filesystem=/tmp:rw \
  org.flatpak.Builder \
  --user \
  --install \
  --force-clean \
  --state-dir="${STATE_DIR}" \
  "${BUILD_DIR}" \
  "${MANIFEST}"

flatpak run --command=syntax-bridge-toolchain-tests "${APP_ID}"
flatpak run --command=syntax-bridge-server-health-tests "${APP_ID}"
flatpak run --command=syntax-bridge-project-ingest-tests "${APP_ID}"
