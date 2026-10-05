#!/usr/bin/env bash
# WHY: Named lanes must use the dev CLI directory derived from the exact instance slug.
# WHY: This guards dc against falling back to the canonical dev instance.

set -euo pipefail
for name in $(compgen -v | grep '^CODETERM_' || true); do unset "$name"; done

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIB="$ROOT/evals/_lib/lib.sh"
FIXTURE="$(mktemp -d)"
trap 'status=$?; rm -rf "$FIXTURE"; exit "$status"' EXIT

mkdir -p \
  "$FIXTURE/home/.codeterm-dev/bin" \
  "$FIXTURE/home/.codeterm-dev-named-instance/bin" \
  "$FIXTURE/home/.codeterm-dev-default-named/bin" \
  "$FIXTURE/home/.codeterm-dev-missing/bin" \
  "$FIXTURE/hostile"

printf '%s\n' 'token = "canonical-token"' > "$FIXTURE/home/.codeterm-dev/config.toml"
printf '%s\n' 'token = "named-token"' > "$FIXTURE/home/.codeterm-dev-named-instance/config.toml"

printf '%s\n' \
  '#!/usr/bin/env bash' \
  'if [ "${1:-}" = identify ] && [ "${2:-}" = --json ] && [ -z "${CODETERM_API_TOKEN:-}" ]; then' \
  '  case "${CODETERM_INSTANCE:-}" in' \
  '    "") data_dir="$HOME/.codeterm-dev"; port=7686; socket=codeterm-dev.sock; instance=null;;' \
  '    default-named) data_dir="$HOME/.codeterm-dev-default-named"; port=7686; socket=codeterm-dev-default-named.sock; instance='\''"default-named"'\'';;' \
  '    missing) data_dir="$HOME/.codeterm-dev-missing"; port=7799; socket=codeterm-dev-missing.sock; instance='\''"missing"'\'';;' \
  '    *) data_dir="$HOME/.codeterm-dev-named-instance"; port=7788; socket=codeterm-dev-named-instance.sock; instance='\''"  Named_Instance  "'\'';;' \
  '  esac' \
  '  printf '\''{"connection":{"data_dir":"%s","host":"127.0.0.1","port":%s,"socket_path":"%s","instance":%s}}\n'\'' "$data_dir" "$port" "$socket" "$instance"' \
  '  exit 0' \
  'fi' \
  'if [ "${1:-}" = config ] && [ "${2:-}" = get ] && [ "${3:-}" = api.token ]; then' \
  '  case "${CODETERM_INSTANCE:-}" in' \
  '    *missing*) echo "Error: CodeTerm socket '\''codeterm-dev-missing.sock'\'' is not live" >&2; exit 1;;' \
  '    *) printf '\''{"key":"api.token","value":"named-token"}\n'\''; exit 0;;' \
  '  esac' \
  'fi' \
  'printf "DEV_CLI_WON token=%s port=%s api_url=%s data_dir=%s vite=%s cdp=%s socket=%s instance=%s\n" "$CODETERM_API_TOKEN" "$CODETERM_API_PORT" "$CODETERM_API_URL" "$CODETERM_DATA_DIR" "$CODETERM_VITE_PORT" "$CODETERM_CDP_PORT" "$CODETERM_SOCKET_PATH" "$CODETERM_INSTANCE"' \
  > "$FIXTURE/home/.codeterm-dev/bin/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'if [ "${1:-}" = identify ] && [ "${2:-}" = --json ]; then' \
  '  printf '\''{"connection":{"data_dir":"%s","host":"127.0.0.1","port":7788,"socket_path":"codeterm-dev-named-instance.sock","instance":"  Named_Instance  "}}\n'\'' "$HOME/.codeterm-dev-named-instance"' \
  '  exit 0' \
  'fi' \
  'if [ "${1:-}" = config ] && [ "${2:-}" = get ] && [ "${3:-}" = api.token ]; then' \
  '  case "${CODETERM_INSTANCE:-}" in' \
  '    *missing*) echo "Error: CodeTerm socket '\''codeterm-dev-missing.sock'\'' is not live" >&2; exit 1;;' \
  '    *) printf '\''{"key":"api.token","value":"named-token"}\n'\''; exit 0;;' \
  '  esac' \
  'fi' \
  'printf "NAMED_DEV_CLI_WON token=%s port=%s api_url=%s data_dir=%s vite=%s cdp=%s socket=%s instance=%s\n" "$CODETERM_API_TOKEN" "$CODETERM_API_PORT" "$CODETERM_API_URL" "$CODETERM_DATA_DIR" "$CODETERM_VITE_PORT" "$CODETERM_CDP_PORT" "$CODETERM_SOCKET_PATH" "$CODETERM_INSTANCE"' \
  > "$FIXTURE/home/.codeterm-dev-named-instance/bin/codeterm"
cp "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-missing/bin/codeterm"
mkdir -p "$FIXTURE/home/.codeterm-dev-wrong/bin"
cp "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-wrong/bin/codeterm"
cp "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-default-named/bin/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  'echo FAKE_PROD_CLI_WON' \
  > "$FIXTURE/hostile/codeterm"
printf '%s\n' \
  '#!/usr/bin/env bash' \
  '[ -z "${CURL_LOG:-}" ] || printf '\''%s\n'\'' "$*" >> "$CURL_LOG"' \
  'body="{\"status\":\"ok\",\"instance\":{\"name\":\"${HEALTH_INSTANCE:-}\",\"api_port\":\"${HEALTH_API_PORT:-7686}\",\"vite_port\":\"${HEALTH_VITE_PORT:-1420}\",\"cdp_port\":\"${HEALTH_CDP_PORT:-9223}\"}}"' \
  'case "$*" in' \
  '  *"%{http_code}"*) printf '\''%s\n200\n'\'' "$body";;' \
  '  *) printf '\''%s\n'\'' "$body";;' \
  'esac' \
  > "$FIXTURE/hostile/curl"
chmod +x \
  "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-named-instance/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-default-named/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-missing/bin/codeterm" \
  "$FIXTURE/home/.codeterm-dev-wrong/bin/codeterm" \
  "$FIXTURE/hostile/codeterm" \
  "$FIXTURE/hostile/curl"

canonical_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    bash -c 'source "$1"; dc identify' _ "$LIB"
)"
if ! grep -Fqx "DEV_CLI_WON token=canonical-token port=7686 api_url=http://127.0.0.1:7686 data_dir=$FIXTURE/home/.codeterm-dev vite=1420 cdp=9223 socket=codeterm-dev.sock instance=" <<<"$canonical_output"; then
  echo "canonical dev CLI did not run: $canonical_output" >&2
  exit 1
fi
if grep -q 'FAKE_PROD_CLI_WON' <<<"$canonical_output"; then
  echo "hostile PATH binary won for the canonical dev instance" >&2
  exit 1
fi

set +e
default_restart_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    bash -c 'source "$1"; dev_assert_restart_target' _ "$LIB" 2>&1
)"
default_restart_status=$?
set -e
if [ "$default_restart_status" -eq 0 ] || ! grep -q 'refusing to restart the default DEV instance' <<<"$default_restart_output"; then
  echo "default DEV restart was not refused: $default_restart_output" >&2
  exit 1
fi

HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
  bash -c 'source "$1"; dev_assert_restart_target 1' _ "$LIB"
chmod -x "$FIXTURE/home/.codeterm-dev/bin/codeterm"
named_cdp="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    CODETERM_INSTANCE='  Named_Instance  ' \
    bash -c 'source "$1"; dev_cdp_port' _ "$LIB"
)"
named_vite="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    CODETERM_INSTANCE='  Named_Instance  ' \
    bash -c 'source "$1"; dev_vite_port' _ "$LIB"
)"

named_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    CODETERM_INSTANCE='  Named_Instance  ' \
    CODETERM_API_TOKEN=prod-token CODETERM_SOCKET_PATH=prod.sock \
    HEALTH_INSTANCE=Named_Instance HEALTH_API_PORT=7788 HEALTH_VITE_PORT="$named_vite" HEALTH_CDP_PORT="$named_cdp" \
    bash -c 'source "$1"; dev_assert_restart_target; dc identify' _ "$LIB"
)"
if ! grep -Fqx "NAMED_DEV_CLI_WON token=named-token port=7788 api_url=http://127.0.0.1:7788 data_dir=$FIXTURE/home/.codeterm-dev-named-instance vite=$named_vite cdp=$named_cdp socket=codeterm-dev-named-instance.sock instance=Named_Instance" <<<"$named_output"; then
  echo "named dev CLI or token selection failed: $named_output" >&2
  exit 1
fi

named_http_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    CODETERM_INSTANCE='  Named_Instance  ' \
    CODETERM_API_TOKEN=prod-token CODETERM_SOCKET_PATH=prod.sock \
    HEALTH_INSTANCE=Named_Instance HEALTH_API_PORT=7788 HEALTH_VITE_PORT="$named_vite" HEALTH_CDP_PORT="$named_cdp" \
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
if ! grep -Fq "resolved_path='$FIXTURE/home/.codeterm-dev-missing/config.toml'" <<<"$missing_output"; then
  echo "resolution error did not name the missing config path: $missing_output" >&2
  exit 1
fi
if ! grep -Fq "expected_data_dir='$FIXTURE/home/.codeterm-dev-missing'" <<<"$missing_output"; then
  echo "resolution error did not name the CLI-expected data directory: $missing_output" >&2
  exit 1
fi
if grep -q 'FAKE_PROD_CLI_WON' <<<"$missing_output"; then
  echo "dc fell back to PATH when the active dev CLI was missing" >&2
  exit 1
fi

set +e
mismatch_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" CODETERM_INSTANCE=wrong \
    bash -c 'source "$1"; dc identify' _ "$LIB" 2>&1
)"
mismatch_status=$?
set -e
if [ "$mismatch_status" -eq 0 ] || ! grep -q 'identify --json disagrees with the resolved target' <<<"$mismatch_output"; then
  echo "dc accepted an identify target that disagreed with the named selector: $mismatch_output" >&2
  exit 1
fi
if grep -q 'DEV_CLI_WON' <<<"$mismatch_output"; then
  echo "dc invoked the requested command after identify reported the wrong target" >&2
  exit 1
fi

set +e
default_named_output="$(
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" CODETERM_INSTANCE=default-named \
    bash -c 'source "$1"; dev_resolve' _ "$LIB" 2>&1
)"
default_named_status=$?
set -e
if [ "$default_named_status" -eq 0 ] || ! grep -q 'named instance resolved to a default API port' <<<"$default_named_output"; then
  echo "bench resolution accepted a named instance on the default DEV port: $default_named_output" >&2
  exit 1
fi

echo "PASS: dc and HTTP resolution is CLI-derived, instance-aware, and fail-closed"
