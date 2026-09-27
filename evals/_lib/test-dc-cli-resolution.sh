#!/usr/bin/env bash
# WHAT: Proves dc and HTTP helpers select the CLI-resolved dev instance.
# WHY: Named lanes received the canonical token and failed with a misleading HTTP 401.
# ORACLE: A deliberately non-canonical instance spelling reaches its fixture CLI and API;
# missing resolution names the instance, resolved path, and expected data directory.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIB="$ROOT/evals/_lib/lib.sh"
FIXTURE="$(mktemp -d)"
trap 'status=$?; rm -rf "$FIXTURE"; exit "$status"' EXIT

mkdir -p \
  "$FIXTURE/home/.codeterm-dev/bin" \
  "$FIXTURE/home/.codeterm-dev-named-instance/bin" \
  "$FIXTURE/home/.codeterm-dev-missing/bin" \
  "$FIXTURE/hostile"

printf '%s\n' 'token = "canonical-token"' > "$FIXTURE/home/.codeterm-dev/config.toml"
printf '%s\n' 'token = "named-token"' > "$FIXTURE/home/.codeterm-dev-named-instance/config.toml"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'if [ "${1:-}" = identify ] && [ "${2:-}" = --json ] && [ -z "${CODETERM_API_TOKEN:-}" ]; then' \
  '  case "${CODETERM_INSTANCE:-}" in' \
  '    "") data_dir="$HOME/.codeterm-dev"; port=7686; instance=null;;' \
  '    missing) data_dir="$HOME/.codeterm-dev-missing"; port=7799; instance='\''"missing"'\'';;' \
  '    *) data_dir="$HOME/.codeterm-dev-named-instance"; port=7788; instance='\''"  Named_Instance  "'\'';;' \
  '  esac' \
  '  printf '\''{"connection":{"data_dir":"%s","host":"127.0.0.1","port":%s,"socket_path":"fixture.sock","instance":%s}}\n'\'' "$data_dir" "$port" "$instance"' \
  '  exit 0' \
  'fi' \
  'if [ "${1:-}" = config ] && [ "${2:-}" = get ] && [ "${3:-}" = api.token ]; then' \
  '  case "${CODETERM_INSTANCE:-}" in' \
  '    *missing*) echo "Error: CodeTerm socket '\''codeterm-dev-missing.sock'\'' is not live" >&2; exit 1;;' \
  '    *) printf '\''{"key":"api.token","value":"named-token"}\n'\''; exit 0;;' \
  '  esac' \
  'fi' \
  'printf "DEV_CLI_WON token=%s port=%s\n" "$CODETERM_API_TOKEN" "$CODETERM_API_PORT"' \
  > "$FIXTURE/home/.codeterm-dev/bin/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'if [ "${1:-}" = identify ] && [ "${2:-}" = --json ]; then' \
  '  printf '\''{"connection":{"data_dir":"%s","host":"127.0.0.1","port":7788,"socket_path":"named.sock","instance":"Named_Instance"}}\n'\'' "$HOME/.codeterm-dev-named-instance"' \
  '  exit 0' \
  'fi' \
  'if [ "${1:-}" = config ] && [ "${2:-}" = get ] && [ "${3:-}" = api.token ]; then' \
  '  case "${CODETERM_INSTANCE:-}" in' \
  '    *missing*) echo "Error: CodeTerm socket '\''codeterm-dev-missing.sock'\'' is not live" >&2; exit 1;;' \
  '    *) printf '\''{"key":"api.token","value":"named-token"}\n'\''; exit 0;;' \
  '  esac' \
  'fi' \
  'printf "NAMED_DEV_CLI_WON token=%s port=%s\n" "$CODETERM_API_TOKEN" "$CODETERM_API_PORT"' \
  > "$FIXTURE/home/.codeterm-dev-named-instance/bin/codeterm"
cp "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-missing/bin/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'echo FAKE_PROD_CLI_WON' \
  > "$FIXTURE/hostile/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  '[ -z "${CURL_LOG:-}" ] || printf '\''%s\n'\'' "$*" >> "$CURL_LOG"' \
  'body='\''{"status":"ok","instance":{"name":"  Named_Instance  ","api_port":"7788"}}'\''' \
  'case "$*" in' \
  '  *"%{http_code}"*) printf '\''%s\n200\n'\'' "$body";;' \
  '  *) printf '\''%s\n'\'' "$body";;' \
  'esac' \
  > "$FIXTURE/hostile/curl"
chmod +x \
  "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-named-instance/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-missing/bin/codeterm" \
  "$FIXTURE/hostile/codeterm" \
  "$FIXTURE/hostile/curl"

canonical_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    bash -c 'source "$1"; dc identify' _ "$LIB"
)"
if ! grep -q '^DEV_CLI_WON token=canonical-token port=7686$' <<<"$canonical_output"; then
  echo "canonical dev CLI did not run: $canonical_output" >&2
  exit 1
fi
if grep -q 'FAKE_PROD_CLI_WON' <<<"$canonical_output"; then
  echo "hostile PATH binary won for the canonical dev instance" >&2
  exit 1
fi
chmod -x "$FIXTURE/home/.codeterm-dev/bin/codeterm"

named_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    CODETERM_INSTANCE='  Named_Instance  ' \
    CODETERM_API_TOKEN=prod-token CODETERM_SOCKET_PATH=prod.sock \
    bash -c 'source "$1"; dc identify' _ "$LIB"
)"
if ! grep -q '^NAMED_DEV_CLI_WON token=named-token port=7788$' <<<"$named_output"; then
  echo "named dev CLI or token selection failed: $named_output" >&2
  exit 1
fi

named_http_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    CODETERM_INSTANCE='  Named_Instance  ' \
    CODETERM_API_TOKEN=prod-token CODETERM_SOCKET_PATH=prod.sock \
    CURL_LOG="$FIXTURE/curl.log" \
    bash -c 'source "$1"; dev_get /api/health' _ "$LIB"
)"
if ! grep -q '"status":"ok"' <<<"$named_http_output"; then
  echo "named HTTP helper did not reach the selected instance: $named_http_output" >&2
  exit 1
fi
if ! grep -q '127.0.0.1:7788/api/health' "$FIXTURE/curl.log"; then
  echo "named HTTP helper ignored the CLI-resolved port" >&2
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
  echo "dc silently succeeded without the requested dev instance" >&2
  exit 1
fi
if ! grep -q "instance='missing'" <<<"$missing_output"; then
  echo "resolution error did not name the requested instance: $missing_output" >&2
  exit 1
fi
if ! grep -q "resolved_path='$FIXTURE/home/.codeterm-dev-missing/config.toml'" <<<"$missing_output"; then
  echo "resolution error did not name the missing config path: $missing_output" >&2
  exit 1
fi
if ! grep -q "expected_data_dir='$FIXTURE/home/.codeterm-dev-missing'" <<<"$missing_output"; then
  echo "resolution error did not name the CLI-expected data directory: $missing_output" >&2
  exit 1
fi
if grep -q 'FAKE_PROD_CLI_WON' <<<"$missing_output"; then
  echo "dc fell back to PATH when the active dev CLI was missing" >&2
  exit 1
fi

echo "PASS: dc and HTTP resolution is CLI-derived, instance-aware, and fail-closed"
