#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
SDK_RUNTIME="${FLATPAK_SDK_RUNTIME:-org.freedesktop.Sdk//25.08}"
RUST_SDK="${FLATPAK_RUST_SDK:-/usr/lib/sdk/rust-stable/enable.sh}"
LLVM_SDK="${FLATPAK_LLVM_SDK:-/usr/lib/sdk/llvm21/enable.sh}"

flatpak run \
  --devel \
  --share=network \
  --filesystem="${ROOT_DIR}:rw" \
  --env=CARGO_HOME=/tmp/syntax-bridge-cargo-home \
  --env=CARGO_TARGET_DIR=/tmp/syntax-bridge-cargo-target \
  --command=sh \
  "${SDK_RUNTIME}" \
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
