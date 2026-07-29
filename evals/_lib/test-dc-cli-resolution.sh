#!/usr/bin/env bash
# WHAT: Proves dc selects the active dev CLI and token without consulting PATH.
# WHY: A stale prod CLI once connected to dev and returned a false-green doctor result.
# ORACLE: Only the fixture dev CLI runs; hostile PATH stays silent and missing CLI fails closed.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIB="$ROOT/evals/_lib/lib.sh"
FIXTURE="$(mktemp -d)"
trap 'rm -rf "$FIXTURE"' EXIT

mkdir -p \
  "$FIXTURE/home/.codeterm-dev/bin" \
  "$FIXTURE/home/.codeterm-dev-named-instance/bin" \
  "$FIXTURE/hostile"

printf '%s\n' 'token = "canonical-token"' > "$FIXTURE/home/.codeterm-dev/config.toml"
printf '%s\n' 'token = "named-token"' > "$FIXTURE/home/.codeterm-dev-named-instance/config.toml"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'printf "DEV_CLI_WON token=%s\n" "$CODETERM_API_TOKEN"' \
  > "$FIXTURE/home/.codeterm-dev/bin/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'printf "NAMED_DEV_CLI_WON token=%s\n" "$CODETERM_API_TOKEN"' \
  > "$FIXTURE/home/.codeterm-dev-named-instance/bin/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'echo FAKE_PROD_CLI_WON' \
  > "$FIXTURE/hostile/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'exit 0' \
  > "$FIXTURE/hostile/curl"
chmod +x \
  "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-named-instance/bin/codeterm" \
  "$FIXTURE/hostile/codeterm" \
  "$FIXTURE/hostile/curl"

canonical_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    bash -c 'source "$1"; dc identify' _ "$LIB"
)"
if ! grep -q '^DEV_CLI_WON token=canonical-token$' <<<"$canonical_output"; then
  echo "canonical dev CLI did not run: $canonical_output" >&2
  exit 1
fi
if grep -q 'FAKE_PROD_CLI_WON' <<<"$canonical_output"; then
  echo "hostile PATH binary won for the canonical dev instance" >&2
  exit 1
fi

named_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" CODETERM_INSTANCE=Named_Instance \
    bash -c 'source "$1"; dc identify' _ "$LIB"
)"
if ! grep -q '^NAMED_DEV_CLI_WON token=named-token$' <<<"$named_output"; then
  echo "named dev CLI or token selection failed: $named_output" >&2
  exit 1
fi
if grep -q 'FAKE_PROD_CLI_WON' <<<"$named_output"; then
  echo "hostile PATH binary won for the named dev instance" >&2
  exit 1
fi

set +e
missing_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" CODETERM_INSTANCE=missing \
    bash -c 'source "$1"; dc identify' _ "$LIB" 2>&1
)"
missing_status=$?
set -e

if [ "$missing_status" -eq 0 ]; then
  echo "dc silently succeeded without the active dev CLI" >&2
  exit 1
fi
if ! grep -q "$FIXTURE/home/.codeterm-dev-missing/bin/codeterm" <<<"$missing_output"; then
  echo "missing-binary error did not name the active dev CLI: $missing_output" >&2
  exit 1
fi
if ! grep -q './build.sh cli' <<<"$missing_output"; then
  echo "missing-binary error omitted the rebuild command: $missing_output" >&2
  exit 1
fi
if grep -q 'FAKE_PROD_CLI_WON' <<<"$missing_output"; then
  echo "dc fell back to PATH when the active dev CLI was missing" >&2
  exit 1
fi

echo "PASS: dc CLI resolution is absolute, instance-aware, and fail-closed"
