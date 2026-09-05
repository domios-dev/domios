#!/usr/bin/env bash
# WHAT: Proves sourcing the bench lib leaves the caller's shell untouched while
# dc stays locked to dev.
# WHY: Sourcing exported five CODETERM_* vars, so a shell that ran one bench
# stayed dev-pointed afterwards and a later bare `codeterm` meant something else.
# ORACLE: the environment and the shell's own CODETERM_* names are identical
# across the source; dc still resolves the dev port, refuses to invoke the CLI
# when dev is not answering, and hands that CLI the dev connection even when the
# parent shell exported the prod port.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
LIB="$ROOT/evals/_lib/lib.sh"
FIXTURE="$(mktemp -d)"
trap 'status=$?; rm -rf "$FIXTURE"; exit "$status"' EXIT

DEV_PORT=7686
PROD_PORT=7685

mkdir -p "$FIXTURE/home/.codeterm-dev/bin" "$FIXTURE/hostile"
printf '%s\n' 'token = "dev-token"' > "$FIXTURE/home/.codeterm-dev/config.toml"

# The dev CLI answers resolution, then reports the connection it was handed.
cat > "$FIXTURE/home/.codeterm-dev/bin/codeterm" <<'CLI'
#!/usr/bin/env bash
if [ "${1:-}" = identify ] && [ "${2:-}" = --json ]; then
  printf '{"connection":{"data_dir":"%s","host":"127.0.0.1","port":7686,"socket_path":"dev.sock","instance":null}}\n' \
    "$HOME/.codeterm-dev"
  exit 0
fi
printf 'CLI_SAW port=%s token=%s socket=%s tab=%s\n' \
  "${CODETERM_API_PORT:-unset}" "${CODETERM_API_TOKEN:-unset}" \
  "${CODETERM_SOCKET_PATH:-unset}" "${CODETERM_TAB_ID-unset}"
CLI

cat > "$FIXTURE/hostile/curl" <<'CURL'
#!/usr/bin/env bash
[ -z "${CURL_LOG:-}" ] || printf '%s\n' "$*" >> "$CURL_LOG"
[ "${DEV_DOWN:-0}" != 1 ] || exit 7
printf '{"status":"ok"}\n'
CURL

cat > "$FIXTURE/hostile/codeterm" <<'PROD'
#!/usr/bin/env bash
echo FAKE_PROD_CLI_WON
PROD

cat > "$FIXTURE/child-probe" <<'PROBE'
#!/usr/bin/env bash
printf 'CHILD port=%s auth=%s\n' \
  "${CODETERM_API_PORT:-unset}" "${CODETERM_DEV_AUTH_HEADER:-unset}"
PROBE

chmod +x \
  "$FIXTURE/home/.codeterm-dev/bin/codeterm" \
  "$FIXTURE/hostile/curl" \
  "$FIXTURE/hostile/codeterm" \
  "$FIXTURE/child-probe"

fail() { echo "FAIL: $*" >&2; exit 1; }

# This bench runs inside CodeTerm, whose own CODETERM_* variables would other-
# wise decide the result. Every probe starts from a shell that holds none.
SCRUB='for v in $(compgen -v | grep "^CODETERM_" || true); do unset "$v"; done; '
POISON='export CODETERM_API_PORT='"$PROD_PORT"' CODETERM_API_TOKEN=prod-token \
  CODETERM_SOCKET_PATH=prod.sock CODETERM_TAB_ID=prod-tab; '

# Capture the CODETERM_* names this shell exports and the ones it merely holds,
# on both sides of the source. A non-exported plant is still a plant.
CAPTURE='
  env | grep "^CODETERM_" | sort > "$2/before.env" || true
  compgen -v | grep "^CODETERM_" | sort > "$2/before.vars" || true
  source "$1"
  env | grep "^CODETERM_" | sort > "$2/after.env" || true
  compgen -v | grep "^CODETERM_" | sort > "$2/after.vars" || true
'

probe() {
  HOME="$FIXTURE/home" PATH="$FIXTURE/hostile:$PATH" \
    CURL_LOG="${CURL_LOG:-}" DEV_DOWN="${DEV_DOWN:-0}" \
    bash -c "$SCRUB$1" _ "$LIB" "$FIXTURE"
}

assert_source_is_inert() {
  local label="$1"
  if ! diff -u "$FIXTURE/before.env" "$FIXTURE/after.env" > "$FIXTURE/env.diff"; then
    fail "$label: sourcing the lib changed the exported environment:
$(cat "$FIXTURE/env.diff")"
  fi
  if ! diff -u "$FIXTURE/before.vars" "$FIXTURE/after.vars" > "$FIXTURE/vars.diff"; then
    fail "$label: sourcing the lib planted a CODETERM_* shell variable:
$(cat "$FIXTURE/vars.diff")"
  fi
}

# --- 1. A clean shell gains nothing ----------------------------------
probe "$CAPTURE"
assert_source_is_inert "clean parent"
if [ -s "$FIXTURE/after.env" ]; then
  fail "a clean shell holds CODETERM_* after sourcing: $(cat "$FIXTURE/after.env")"
fi
if [ -s "$FIXTURE/after.vars" ]; then
  fail "a clean shell holds CODETERM_* names after sourcing: $(cat "$FIXTURE/after.vars")"
fi

# --- 2. A poisoned parent keeps its own values, unedited -------------
probe "$POISON$CAPTURE"
assert_source_is_inert "poisoned parent"
if ! grep -qx "CODETERM_API_PORT=$PROD_PORT" "$FIXTURE/after.env"; then
  fail "the parent's own CODETERM_API_PORT did not survive sourcing"
fi

# --- 3. dc targets dev and hands the CLI the dev connection ----------
CURL_LOG="$FIXTURE/curl.log"
dev_output="$(probe 'source "$1"; dc whoami')"
if ! grep -q "^CLI_SAW port=$DEV_PORT token=dev-token socket=dev.sock tab=$" <<<"$dev_output"; then
  fail "dc did not compose the dev connection into the CLI process: $dev_output"
fi
if grep -q FAKE_PROD_CLI_WON <<<"$dev_output"; then
  fail "dc resolved the CLI through PATH"
fi
if ! grep -q "127.0.0.1:$DEV_PORT/api/health" "$FIXTURE/curl.log"; then
  fail "dc did not assert dev liveness before invoking the CLI"
fi

# --- 4. A poisoned parent cannot redirect dc off dev -----------------
CURL_LOG="$FIXTURE/poisoned-curl.log"
poisoned_output="$(probe "$POISON"'source "$1"; dc whoami')"
if ! grep -q "^CLI_SAW port=$DEV_PORT token=dev-token socket=dev.sock tab=$" <<<"$poisoned_output"; then
  fail "a leaked parent CODETERM_* reached the CLI: $poisoned_output"
fi
if grep -q "127.0.0.1:$PROD_PORT" "$FIXTURE/poisoned-curl.log"; then
  fail "dc probed the prod port"
fi

# --- 5. --as-tab is the only way a tab id reaches the CLI ------------
CURL_LOG="$FIXTURE/as-tab-curl.log"
as_tab_output="$(probe "$POISON"'source "$1"; dc --as-tab dev-tab whoami')"
if ! grep -q "tab=dev-tab$" <<<"$as_tab_output"; then
  fail "--as-tab did not reach the CLI: $as_tab_output"
fi

# --- 6. dc refuses to invoke the CLI when dev is not answering -------
CURL_LOG="$FIXTURE/down-curl.log"
DEV_DOWN=1
set +e
down_output="$(probe 'source "$1"; dc whoami' 2>&1)"
down_status=$?
set -e
DEV_DOWN=0
if [ "$down_status" -eq 0 ]; then
  fail "dc succeeded while dev was unreachable"
fi
if grep -q CLI_SAW <<<"$down_output"; then
  fail "dc invoked the CLI before proving dev was alive: $down_output"
fi
if grep -q FAKE_PROD_CLI_WON <<<"$down_output"; then
  fail "dc fell back to the PATH binary when dev was unreachable"
fi

# --- 7. dev_run composes the connection for a non-CLI child ----------
CURL_LOG="$FIXTURE/child-curl.log"
child_output="$(probe "$POISON"'source "$1"; dev_run "$2/child-probe"')"
if ! grep -q "^CHILD port=$DEV_PORT auth=Authorization: Bearer dev-token$" <<<"$child_output"; then
  fail "dev_run did not compose the dev connection for its child: $child_output"
fi

echo "PASS: sourcing the bench lib exports nothing and dc cannot be redirected off dev"
