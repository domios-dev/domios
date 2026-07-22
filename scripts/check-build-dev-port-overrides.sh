#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

assert_output() {
  local expected="$1" actual="$2"
  if [ "$actual" != "$expected" ]; then
    echo "unexpected build.sh dry-run output" >&2
    printf 'expected: %s\nactual:   %s\n' "$expected" "$actual" >&2
    exit 1
  fi
}

explicit_output="$(
  CODETERM_TEST_KILL_DRY_RUN=1 \
  CODETERM_INSTANCE=playwright-harness \
  CODETERM_API_PORT=7776 \
  CODETERM_VITE_PORT=1736 \
  CODETERM_CDP_PORT=9736 \
    ./build.sh kill
)"
assert_output \
  $'Dry-run dev kill instance=\'playwright-harness\' api=7776 vite=1736 cdp=9736 identifier=com.codeterm.dev.playwright.harness\nDone.' \
  "$explicit_output"

default_output="$(
  CODETERM_TEST_KILL_DRY_RUN=1 \
  CODETERM_API_PORT=7778 \
  CODETERM_VITE_PORT=1737 \
  CODETERM_CDP_PORT=9737 \
    ./build.sh kill
)"
assert_output \
  $'Dry-run dev kill instance=\'default\' api=7778 vite=1737 cdp=9737 identifier=com.codeterm.dev\nDone.' \
  "$default_output"

fallback_output="$(
  CODETERM_TEST_KILL_DRY_RUN=1 \
  CODETERM_INSTANCE=port-check \
  CODETERM_API_PORT=7777 \
    ./build.sh kill
)"
assert_output \
  $'Dry-run dev kill instance=\'port-check\' api=7777 vite=1429 cdp=9232 identifier=com.codeterm.dev.port.check\nDone.' \
  "$fallback_output"

set +e
prod_output="$(
  CODETERM_TEST_KILL_DRY_RUN=1 \
  CODETERM_INSTANCE=prod \
  CODETERM_API_PORT=7685 \
    ./build.sh kill 2>&1
)"
prod_status=$?
set -e
if [ "$prod_status" -eq 0 ] || ! grep -Eq 'refusing to run kill against prod port 7685' <<< "$prod_output"; then
  echo "named instance on prod port 7685 must remain refused" >&2
  echo "$prod_output" >&2
  exit 1
fi

echo "build dev port override check passed"
