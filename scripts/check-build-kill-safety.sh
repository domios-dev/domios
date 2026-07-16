#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

prod_inherited_output="$(CODETERM_TEST_KILL_DRY_RUN=1 CODETERM_API_PORT=7685 ./build.sh kill 2>&1)"
if ! grep -q "Done." <<< "$prod_inherited_output"; then
  echo "expected inherited prod port to fall back to canonical dev kill" >&2
  echo "$prod_inherited_output" >&2
  exit 1
fi
filtered_prod_output="$(
  printf '%s\n' "$prod_inherited_output" \
    | grep -v 'ignoring inherited CODETERM_API_PORT=7685' \
    | grep -v 'ignoring inherited prod CodeTerm port' \
    || true
)"
if grep -q "7685" <<< "$filtered_prod_output"; then
  echo "canonical kill path must not mention or target prod port 7685" >&2
  echo "$prod_inherited_output" >&2
  exit 1
fi

kill_dev_body="$(sed -n '/^kill_dev() {/,/^}/p' build.sh)"
mapfile -t request_lines < <(grep -n 'request_dev_clean_restart' <<< "$kill_dev_body" | cut -d: -f1)
mapfile -t watcher_lines < <(grep -n 'kill_web_watch' <<< "$kill_dev_body" | cut -d: -f1)
if [ "${#request_lines[@]}" -ne 3 ] || [ "${#watcher_lines[@]}" -ne 2 ] \
  || [ "${request_lines[1]}" -ge "${watcher_lines[0]}" ] \
  || [ "${request_lines[2]}" -ge "${watcher_lines[1]}" ]; then
  echo "dev kill must request clean daemon shutdown before stopping the web watcher" >&2
  exit 1
fi

set +e
named_prod_output="$(CODETERM_INSTANCE=prod CODETERM_API_PORT=7685 ./build.sh kill 2>&1)"
named_prod_status=$?
set -e

if [ "$named_prod_status" -eq 0 ]; then
  echo "expected named instance on prod port to be refused" >&2
  echo "$named_prod_output" >&2
  exit 1
fi
if ! grep -Eq 'refusing to (target prod CodeTerm port|run kill against prod port) 7685' <<< "$named_prod_output"; then
  echo "expected explicit prod refusal message" >&2
  echo "$named_prod_output" >&2
  exit 1
fi

default_policy="$(CODETERM_TEST_PRINT_BUILD_CACHE_POLICY=1 ./build.sh gateway-instance-policy)"
named_policy="$(CODETERM_INSTANCE=provider-model-oracle CODETERM_API_PORT=7787 CODETERM_TEST_PRINT_BUILD_CACHE_POLICY=1 ./build.sh gateway-instance-policy)"
grep -q "identifier=com.codeterm.dev$" <<< "$default_policy" || {
  echo "default instance identifier changed unexpectedly" >&2
  echo "$default_policy" >&2
  exit 1
}
grep -q "identifier=com.codeterm.dev.provider.model.oracle" <<< "$named_policy" || {
  echo "named instance must have its own WebView2 identifier" >&2
  echo "$named_policy" >&2
  exit 1
}
grep -q "api_port=7787" <<< "$named_policy" || {
  echo "named instance must preserve requested API port" >&2
  echo "$named_policy" >&2
  exit 1
}
if grep -q "CARGO_TARGET_DIR=.*codeterm-worktrees" <<< "$named_policy"; then
  echo "default policy must not inject a shared canonical target dir" >&2
  echo "$named_policy" >&2
  exit 1
fi

named_gateway_policy="$(CODETERM_INSTANCE=pw1 CODETERM_API_PORT=7786 CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait)"
canonical_gateway_policy="$(CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait)"
grep -q "shared_gateway_handoff=skipped" <<< "$named_gateway_policy" || {
  echo "named dev --wait must structurally skip the shared build-gateway handoff" >&2
  exit 1
}
grep -q "shared_gateway_handoff=required" <<< "$canonical_gateway_policy" || {
  echo "canonical dev --wait must retain the shared build-gateway handoff" >&2
  exit 1
}
if CODETERM_INSTANCE=pw1 CODETERM_API_PORT=7786 CODETERM_TEST_PRINT_DEV_GATEWAY_POLICY=1 ./build.sh dev --wait --release-gateway-after-ready >/dev/null 2>&1; then
  echo "obsolete caller-controlled gateway handoff flag must be rejected" >&2
  exit 1
fi

marker_fixture="$(mktemp -d)"
trap 'rm -rf "$marker_fixture"' EXIT
mkdir -p "$marker_fixture/home/.codeterm-dev-marker-test" "$marker_fixture/bin"
printf '[api]\ntoken = "matching-instance-token"\n' > "$marker_fixture/home/.codeterm-dev-marker-test/config.toml"
cat > "$marker_fixture/bin/curl" <<'EOF'
#!/usr/bin/env bash
printf '%s\n' "$*" > "$CODETERM_TEST_CURL_LOG"
EOF
chmod +x "$marker_fixture/bin/curl"

HOME="$marker_fixture/home" \
PATH="$marker_fixture/bin:$PATH" \
CODETERM_INSTANCE=marker-test \
CODETERM_API_PORT=7799 \
CODETERM_TEST_KILL_CLEAN_REQUEST_ONLY=1 \
CODETERM_TEST_CURL_LOG="$marker_fixture/curl.log" \
  ./build.sh kill >/dev/null

grep -q 'Authorization: Bearer matching-instance-token' "$marker_fixture/curl.log" || {
  echo "dev kill must authenticate with the matching instance token" >&2
  exit 1
}
grep -q 'POST http://127.0.0.1:7799/api/debug/shutdown/clean-restart' "$marker_fixture/curl.log" || {
  echo "dev kill must request a clean restart from the matching dev instance" >&2
  exit 1
}

powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/check-windows-dev-cleanup-selection.ps1

echo "build kill safety check passed"
