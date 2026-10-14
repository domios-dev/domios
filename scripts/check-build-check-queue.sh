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
warning_log="$(printf '%s\n' \
  'src-tauri/crates/codeterm-util/src/a.rs:1:5: warning: unused import: `x`' \
  'src-tauri/crates/codeterm-util/src/a.rs:1:5: warning: unused import: `x`' \
  'src/main.rs:3:9: warning: variable does not need to be mutable' \
  'warning: `codeterm-util` (lib) generated 1 warning' \
  'warning: `codeterm-util` (lib test) generated 1 warning (1 duplicate)' \
  'error: could not compile `codeterm` (lib) due to 1 previous error; 2 warnings emitted' \
  'warning: `codeterm-cli` (bin "codeterm") generated 1 warning (run `cargo clippy --fix --bin "codeterm"` to apply 1 suggestion)')"

report="$(printf '%s\n' "$warning_log" | { ./build.sh check-queue-probe warnings 1 "codeterm-plan codeterm-util" || true; } | grep -Ev '^MSVC env')"
[ "$(printf '%s\n' "$report" | sed -n 1p)" = 'src-tauri/crates/codeterm-util/src/a.rs:1:5: warning: unused import: `x`' ] || { echo "first warning missing: $report" >&2; exit 1; }
printf '%s\n' "$report" | grep -qx 'warnings crate=codeterm-util count=1' || { echo "duplicate not subtracted: $report" >&2; exit 1; }
printf '%s\n' "$report" | grep -qx 'warnings crate=codeterm count=2' || { echo "failed-unit warnings not counted: $report" >&2; exit 1; }
[ "$(printf '%s\n' "$report" | tail -n 1)" = 'warnings=2 shown=1 failing=codeterm-util' ] || { echo "warning summary wrong: $report" >&2; exit 1; }
if printf '%s\n' "$warning_log" | ./build.sh check-queue-probe warnings 1 "codeterm-plan codeterm-util" >/dev/null; then
  echo "touched crate warnings did not fail the run" >&2; exit 1
fi

printf '%s\n' "$warning_log" | ./build.sh check-queue-probe warnings 5 "codeterm-plan" >/dev/null || { echo "untouched crate warnings failed the run" >&2; exit 1; }
if printf '%s\n' "$warning_log" | ./build.sh check-queue-probe warnings 5 "codeterm-cli" >/dev/null; then
  echo "touched cli warnings did not fail the run" >&2; exit 1
fi
if printf '%s\n' "$warning_log" | ./build.sh check-queue-probe warnings 5 "*" >/dev/null; then
  echo "--all warnings did not fail the run" >&2; exit 1
fi
printf 'warning: `codeterm-util` (lib) generated 1 warning (1 duplicate)\n' | ./build.sh check-queue-probe warnings 5 "*" >/dev/null || { echo "fully duplicated warnings failed the run" >&2; exit 1; }

summary="$(printf '%s\n' "$warning_log" | ./build.sh check-queue-probe summary | grep -Ev '^MSVC env')"
[ "$(printf '%s\n' "$summary" | tail -n 1)" = 'check: warnings=4 crates_with_warnings=3' ] || { echo "check summary wrong: $summary" >&2; exit 1; }
[ "$(printf '' | ./build.sh check-queue-probe summary | grep -Ev '^MSVC env')" = 'check: warnings=0 crates_with_warnings=0' ] || { echo "clean check summary wrong" >&2; exit 1; }
echo "check-queue helpers: PASS"
