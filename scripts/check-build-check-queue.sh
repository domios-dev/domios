#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

targets="$(printf '%s\n' \
  src-tauri/crates/codeterm-plan/src/lib.rs \
  src-tauri/crates/codeterm-plan/Cargo.toml \
  src-tauri/src/service/mod.rs \
  cli/src/main.rs \
  src/App.tsx \
  | ./build.sh check-queue-probe targets | grep -Ev '^MSVC env')"
[ "$targets" = "$(printf 'app codeterm\napp codeterm-plan\ncli')" ] || {
  echo "check-queue target mapping regression: $targets" >&2
  exit 1
}

[ -z "$(printf '%s\n' src/App.tsx docs/x.md | ./build.sh check-queue-probe targets | grep -Ev '^MSVC env')" ] || {
  echo "check-queue mapped a non-Rust path" >&2
  exit 1
}

report="$(printf '%s\n' \
  'src-tauri/src/a.rs:10:5: error[E0425]: cannot find value `x` in this scope' \
  'src-tauri/src/a.rs:11:5: warning: unused variable: `y`' \
  'src-tauri/src/b.rs:3:1: error[E0432]: unresolved import `foo`' \
  'src-tauri/src/c.rs:9:9: error[E0412]: cannot find type `T` in this scope' \
  'error: could not compile `codeterm` (lib) due to 3 previous errors' \
  | ./build.sh check-queue-probe errors 2 | grep -Ev '^MSVC env')"
[ "$(printf '%s\n' "$report" | sed -n 1p)" = 'src-tauri/src/a.rs:10:5: error[E0425]: cannot find value `x` in this scope' ] || { echo "first error missing" >&2; exit 1; }
[ "$(printf '%s\n' "$report" | tail -n 1)" = 'errors=3 shown=2' ] || { echo "bound/total summary wrong: $report" >&2; exit 1; }
echo "check-queue helpers: PASS"
