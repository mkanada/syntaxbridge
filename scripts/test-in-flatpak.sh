#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
# Runs against the installed app, not the bare `org.freedesktop.Sdk` runtime:
# the app's own runtime *is* `org.freedesktop.Sdk` (see the manifest), so
# `--devel` mounts the same `rust-stable`/`llvm21` SDK extensions either way,
# but only the installed app's own `/app` prefix has the `dart-sdk` and
# `googletest` modules the test suite needs (`dart format`, `<gtest/gtest.h>`)
# — the bare SDK runtime never had them, so tests exercising either always
# failed under it regardless of how the rest of the build went.
APP_ID="${FLATPAK_APP_ID:-dev.syntax_bridge.SyntaxBridge}"
RUST_SDK="${FLATPAK_RUST_SDK:-/usr/lib/sdk/rust-stable/enable.sh}"
LLVM_SDK="${FLATPAK_LLVM_SDK:-/usr/lib/sdk/llvm21/enable.sh}"

if ! flatpak info "${APP_ID}" >/dev/null 2>&1; then
  echo "error: ${APP_ID} is not installed — run 'just package-build' first" >&2
  echo "(test-in-flatpak.sh runs against the installed app so the dart-sdk/googletest modules it bundles are on PATH, not just the bare SDK runtime)" >&2
  exit 1
fi

# CARGO_HOME/CARGO_TARGET_DIR live under the repo's own (gitignored) `target/`
# rather than the sandbox's private `/tmp`: Flatpak sizes that `/tmp` tmpfs to
# match `$XDG_RUNTIME_DIR` (systemd-logind's `RuntimeDirectorySize`, 10% of
# RAM by default), which is often too small to hold a full workspace debug
# build's artifacts. `target/flatpak` is a separate subdirectory from the
# host build's own `target/` (different toolchain/rustc — sharing one
# directory between the two risks incremental-compilation cache conflicts),
# and lands on the real disk via the `--filesystem` bind mount below instead
# of the size-capped tmpfs.
flatpak run \
  --devel \
  --share=network \
  --filesystem="${ROOT_DIR}:rw" \
  --env=CARGO_HOME="${ROOT_DIR}/target/flatpak/cargo-home" \
  --env=CARGO_TARGET_DIR="${ROOT_DIR}/target/flatpak/cargo-target" \
  --command=sh \
  "${APP_ID}" \
  -c '
set -eu
repo_dir="$1"
rust_sdk="$2"
llvm_sdk="$3"
shift 3

. "$rust_sdk"
. "$llvm_sdk"

cd "$repo_dir"
cargo --offline test "$@"
' \
  sh "${ROOT_DIR}" "${RUST_SDK}" "${LLVM_SDK}" "$@"
