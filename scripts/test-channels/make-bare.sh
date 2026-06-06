#!/usr/bin/env bash
set -euo pipefail

name="${1:?usage: make-bare.sh <name>}"
root="${CODETERM_TEST_CHANNEL_ROOT:-/tmp/codeterm-test-channels}"
work="$root/$name"
bare="$root/$name.git"
repo_root="$(cd "$(dirname "$0")/../.." && pwd)"

rm -rf "$work" "$bare"
mkdir -p "$root"
git init --bare "$bare" >/dev/null
git clone "$bare" "$work" >/dev/null 2>&1
cp -R "$repo_root/plugins" "$work/plugins"
git -C "$work" checkout -b main >/dev/null 2>&1
git -C "$work" add plugins
git -C "$work" commit -m "seed plugin channel $name" >/dev/null
git -C "$work" push origin main >/dev/null

printf 'file://%s#main:plugins\n' "$bare"
