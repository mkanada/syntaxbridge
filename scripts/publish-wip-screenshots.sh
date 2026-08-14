#!/usr/bin/env bash
set -euo pipefail

# Publishes the current working tree's UI as screenshots to a Gist, so it
# can be checked from a phone via the GitHub app/browser while a change is
# still in progress — no commit, branch, or PR in the main repository. See
# AGENTS.md ("just screenshots-wip") and docs/plans/User Steps.md.
#
# `gh gist create`/`gh gist edit` refuse binary files ("binary file not
# supported"), so PNGs are pushed with plain `git` against the gist's own
# git repo instead (every gist is a tiny git repo) — the Gist API only
# rejects binary content on the JSON-payload path, not on git push.
#
# State (the clone + which gist it is) lives under .wip-screenshots/, which
# is gitignored: rerunning this script updates the same gist in place, so
# the URL stays stable across a whole task.

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

STATE_DIR=".wip-screenshots"
CLONE_DIR="$STATE_DIR/gist"
SCREENSHOT_DIR="client/flutter/build/test-screenshots"

git_remote() {
  git -C "$CLONE_DIR" -c credential.helper= -c credential.helper='!gh auth git-credential' "$@"
}

just flutter-screenshots >/dev/null

shopt -s nullglob
pngs=("$SCREENSHOT_DIR"/*.png)
if [ ${#pngs[@]} -eq 0 ]; then
  echo "No screenshots captured; nothing to publish." >&2
  exit 1
fi

mkdir -p "$STATE_DIR"

if [ ! -d "$CLONE_DIR/.git" ]; then
  placeholder_dir="$(mktemp -d)"
  trap 'rm -rf "$placeholder_dir"' EXIT
  cat >"$placeholder_dir/README.md" <<'EOF'
# Syntax Bridge — WIP screenshots

Published on demand while iterating on the UI (`just screenshots-wip`).
Overwritten on every run — not part of the project's commit history.
EOF
  create_output="$(gh gist create --desc 'Syntax Bridge — WIP screenshots (ephemeral)' "$placeholder_dir/README.md" 2>&1)"
  gist_url="$(printf '%s\n' "$create_output" | grep -Eo 'https://gist\.github\.com/\S+' | tail -n1)"
  if [ -z "$gist_url" ]; then
    echo "Could not determine the created gist's URL. gh output was:" >&2
    printf '%s\n' "$create_output" >&2
    exit 1
  fi
  gist_id="${gist_url##*/}"
  gh gist clone "$gist_id" "$CLONE_DIR" >/dev/null
  rm -rf "$placeholder_dir"
  trap - EXIT
else
  git_remote pull --quiet
fi

find "$CLONE_DIR" -maxdepth 1 -name '*.png' -delete
cp "${pngs[@]}" "$CLONE_DIR/"
cat >"$CLONE_DIR/README.md" <<EOF
# Syntax Bridge — WIP screenshots

Published on demand while iterating on the UI (\`just screenshots-wip\`).
Overwritten on every run — not part of the project's commit history.

Captured $(date -u +"%Y-%m-%dT%H:%M:%SZ") from the working tree, before any
commit.
EOF

git -C "$CLONE_DIR" add -A
if git -C "$CLONE_DIR" diff --cached --quiet; then
  echo "No screenshot changes since the last publish."
else
  git -C "$CLONE_DIR" -c user.name='syntax-bridge-wip' -c user.email='wip@localhost' commit --quiet -m 'Update WIP screenshots'
  git_remote push --quiet
fi

gist_url="$(git -C "$CLONE_DIR" remote get-url origin)"
echo "WIP screenshots: ${gist_url%.git}"
