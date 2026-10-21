#!/usr/bin/env bash
# Caller: build.sh script-guards
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
  CODETERM_DEV_INSTANCE=playwright-harness \
  CODETERM_DEV_API_PORT=7776 \
  CODETERM_DEV_VITE_PORT=1736 \
  CODETERM_DEV_CDP_PORT=9736 \
    ./build.sh kill
)"
assert_output \
  $'Dry-run dev kill instance=\'playwright-harness\' api=7776 vite=1736 cdp=9736 identifier=com.codeterm.dev.playwright.harness\nDone.' \
  "$explicit_output"

default_output="$(
  canonical_bin="$(mktemp -d)"
  trap 'rm -rf "$canonical_bin"' EXIT
  real_git="$(command -v git)"
  cat > "$canonical_bin/git" <<EOF
#!/usr/bin/env bash
case "\$*" in
  *--git-dir|*--git-common-dir) printf '%s\n' "$ROOT/.git" ;;
  *) exec "$real_git" "\$@" ;;
esac
EOF
  chmod +x "$canonical_bin/git"
  export PATH="$canonical_bin:$PATH"
  unset CODETERM_DEV_INSTANCE
  CODETERM_TEST_KILL_DRY_RUN=1 \
  CODETERM_DEV_API_PORT=7778 \
  CODETERM_DEV_VITE_PORT=1737 \
  CODETERM_DEV_CDP_PORT=9737 \
    ./build.sh kill
)"
assert_output \
  $'Dry-run dev kill instance=\'default\' api=7778 vite=1737 cdp=9737 identifier=com.codeterm.dev\nDone.' \
  "$default_output"

fallback_output="$(
  env -u CODETERM_DEV_VITE_PORT -u CODETERM_DEV_CDP_PORT \
  CODETERM_TEST_KILL_DRY_RUN=1 \
  CODETERM_DEV_INSTANCE=port-check \
  CODETERM_DEV_API_PORT=7777 \
    ./build.sh kill
)"
assert_output \
  $'Dry-run dev kill instance=\'port-check\' api=7777 vite=1429 cdp=9232 identifier=com.codeterm.dev.port.check\nDone.' \
  "$fallback_output"

set +e
prod_output="$(
  CODETERM_TEST_KILL_DRY_RUN=1 \
  CODETERM_DEV_INSTANCE=prod \
  CODETERM_DEV_API_PORT=7685 \
    ./build.sh kill 2>&1
)"
prod_status=$?
set -e
if [ "$prod_status" -eq 0 ] || ! grep -Eq 'development refuses production port 7685' <<< "$prod_output"; then
  echo "named instance on prod port 7685 must remain refused" >&2
  echo "$prod_output" >&2
  exit 1
fi

echo "build dev port override check passed"
