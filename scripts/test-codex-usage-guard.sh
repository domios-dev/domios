#!/usr/bin/env bash
set -euo pipefail

out="${TMPDIR:-/tmp}/codeterm-codex-usage-guard-test"
rustc --edition=2021 --test src-tauri/src/usage/suspicious.rs -o "$out"
"$out"
