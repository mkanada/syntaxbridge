#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
APP_ID="${FLATPAK_APP_ID:-dev.syntax_bridge.SyntaxBridge}"
MANIFEST="${ROOT_DIR}/build-aux/flatpak/dev.syntax_bridge.SyntaxBridge.json"

# The flatpak-builder state dir holds the module cache and the downloaded Dart
# SDK. Keeping it under /tmp meant losing both on every reboot, since
# systemd-tmpfiles empties /tmp at boot ("D /tmp" in /usr/lib/tmpfiles.d/tmp.conf),
# which forced a 222 MB re-download and a full Rust rebuild.
CACHE_ROOT="${FLATPAK_CACHE_ROOT:-${XDG_CACHE_HOME:-${HOME}/.cache}/syntax-bridge}"
BUILD_DIR="${FLATPAK_BUILD_DIR:-${CACHE_ROOT}/flatpak-build}"
STATE_DIR="${FLATPAK_STATE_DIR:-${CACHE_ROOT}/flatpak-state}"

flatpak info org.flatpak.Builder >/dev/null

build_parent="$(dirname -- "${BUILD_DIR}")"
state_parent="$(dirname -- "${STATE_DIR}")"
mkdir -p -- "${build_parent}" "${state_parent}"

(
  cd "${ROOT_DIR}/client/flutter"
  flutter build linux --release
)

# Pack the Rust inputs into a single file source. flatpak-builder cannot cache a
# module that has a "dir" source -- builder_source_dir_checksum() feeds a random
# value into the cache key with the comment "We can't realistically checksum a
# directory, so always rebuild" -- but it does checksum the contents of a "file"
# source. The tar flags below make the archive byte-identical whenever the tree
# is unchanged, so an untouched Rust tree becomes a cache hit instead of a full
# recompile of the dependency graph.
(
  cd "${ROOT_DIR}"
  tar --format=gnu --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner \
    -cf build-aux/flatpak/rust-src.tar \
    Cargo.toml Cargo.lock .cargo crates vendor
)

flatpak run \
  --filesystem="${ROOT_DIR}:rw" \
  --filesystem="${build_parent}:rw" \
  --filesystem="${state_parent}:rw" \
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
