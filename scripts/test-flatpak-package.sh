#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="${FLATPAK_APP_ID:-dev.syntax_bridge.SyntaxBridge}"

"${ROOT_DIR}/scripts/build-flatpak-package.sh"

flatpak run --command=syntax-bridge-toolchain-tests "${APP_ID}"
flatpak run --command=syntax-bridge-server-health-tests "${APP_ID}"
flatpak run --command=syntax-bridge-project-ingest-tests "${APP_ID}"
