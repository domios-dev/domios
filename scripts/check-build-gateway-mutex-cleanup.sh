#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TEST_ROOT="$ROOT/.research/build-gateway-mutex-$$"
STATE="$TEST_ROOT/state"
RELEASE="$TEST_ROOT/release"
EVENTS="$TEST_ROOT/events"
MARKER="codeterm-gateway-mutex-$RANDOM-$$"
mkdir -p "$TEST_ROOT"
: > "$EVENTS"

cleanup() {
  local rc="$?"
  touch "$RELEASE" 2>/dev/null || true
  [ -n "${HOLDER_PID:-}" ] && kill "$HOLDER_PID" 2>/dev/null || true
  [ "$rc" -ne 0 ] || rm -rf "$TEST_ROOT"
  return "$rc"
}
trap cleanup EXIT

list_fixture_compilers() {
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    # Snapshot first, filter second: awk's own argv carries the marker, so a piped ps
    # would list the awk that is about to read it and the guard would match itself.
    local snapshot; snapshot="$(ps -eo pid=,ppid=,command=)"
    FIXTURE_MARKER="$MARKER" awk 'index($0, ENVIRON["FIXTURE_MARKER"])' <<< "$snapshot"
  else
    powershell.exe -NoProfile -Command \
      "\$marker='$MARKER'; Get-CimInstance Win32_Process | Where-Object { \$_.ProcessId -ne \$PID -and \$_.CommandLine -like \"*\$marker*\" } | Select-Object ProcessId,ParentProcessId,Name,CommandLine | Format-List" |
      tr -d '\r'
  fi
}

CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$STATE" \
CODETERM_GATEWAY_FAKE_EVENTS="$EVENTS" \
CODETERM_GATEWAY_FAKE_WAIT_SECS=150 \
CT_BUILD_JOB_ID=mutex-holder \
CODETERM_BUILD_WORKTREE_ID=unrelated-worktree \
./build.sh gateway-fake holder "wait:$RELEASE" success > "$TEST_ROOT/holder.log" 2>&1 &
HOLDER_PID=$!

node - "$EVENTS" <<'JS'
const fs = require("node:fs");
const file = process.argv[2];
const ready = () => {
  if (fs.readFileSync(file, "utf8").includes("holder start")) process.exit(0);
};
ready();
fs.watch(file, ready);
JS

source scripts/build-gateway.sh
ct_gateway_set_fixture_dir "$STATE"
holder_start="$(ct_gateway_process_start_token "$HOLDER_PID")"
mkdir "$STATE/state.lock"
printf '%s\n' "$HOLDER_PID" > "$STATE/state.lock/pid"
printf '%s\n' "$holder_start" > "$STATE/state.lock/start_token"
unset CT_BUILD_GATEWAY_SCOPE CT_BUILD_GATEWAY_FIXTURE_DIR

set +e
CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$STATE" ./build.sh gateway-status > "$TEST_ROOT/status-timeout.log" 2>&1
status_rc=$?
set -e
cat "$TEST_ROOT/status-timeout.log"
[ "$status_rc" -eq 75 ] || { echo "gateway-status exited $status_rc, expected 75" >&2; exit 1; }

# A symlink, not a copy, on Unix: Homebrew node is dylib-linked via @rpath, and a
# copied binary loses its ../lib. Windows node.exe is self-contained, so copy.
place_fake_compiler() {
  if [[ "$1" == *.exe ]]; then cp "$1" "$2.exe"; else ln -s "$1" "$2"; fi
}
node_path="$(command -v node)"
cargo_bin="$TEST_ROOT/cargo"
[[ "$node_path" != *.exe ]] || cargo_bin="$cargo_bin.exe"
place_fake_compiler "$node_path" "$TEST_ROOT/cargo"
rustc_bin="$TEST_ROOT/rustc"
[[ "$node_path" != *.exe ]] || rustc_bin="$rustc_bin.exe"
place_fake_compiler "$node_path" "$TEST_ROOT/rustc"

set +e
CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$STATE" \
CODETERM_GATEWAY_FAKE_EVENTS="$EVENTS" \
CT_BUILD_JOB_ID=mutex-contender \
CODETERM_BUILD_WORKTREE_ID=failing-worktree \
TEST_CARGO_BIN="$cargo_bin" TEST_RUSTC_BIN="$rustc_bin" TEST_CARGO_MARKER="$MARKER" \
bash -c '
  set -euo pipefail
  coproc CARGO_CHILD {
    "$TEST_CARGO_BIN" scripts/fixtures/build-gateway-compiler-child.mjs \
      cargo "$TEST_RUSTC_BIN" "$TEST_CARGO_MARKER"
  }
  read -r ready <&"${CARGO_CHILD[0]}"
  echo "descendants before mutex timeout:"
  if [[ "$OSTYPE" == darwin* ]] || [[ "$OSTYPE" == linux* ]]; then
    ps -eo pid=,ppid=,command= | awk -v marker="$TEST_CARGO_MARKER" "index(\$0, marker)"
  else
    powershell.exe -NoProfile -Command \
      "\$marker=\"$TEST_CARGO_MARKER\"; Get-CimInstance Win32_Process | Where-Object { \$_.ProcessId -ne \$PID -and \$_.CommandLine -like \"*\$marker*\" } | Select-Object ProcessId,ParentProcessId,Name,CommandLine | Format-List" |
      tr -d "\r"
  fi
  exec ./build.sh gateway-fake contender 0 success
' > "$TEST_ROOT/contender.log" 2>&1
contender_rc=$?
set -e

cat "$TEST_ROOT/contender.log"
[ "$contender_rc" -eq 75 ] || { echo "mutex contender exited $contender_rc, expected 75" >&2; exit 1; }
grep -q "gateway-status" "$TEST_ROOT/contender.log"
grep -q "gateway-cancel" "$TEST_ROOT/contender.log"

echo "descendants after mutex timeout:"
post_timeout="$(list_fixture_compilers)"
printf '%s' "$post_timeout"
[ -z "$post_timeout" ] || { echo "cargo descendant survived mutex timeout" >&2; exit 1; }
kill -0 "$HOLDER_PID"
echo "unrelated worktree holder survived: pid=$HOLDER_PID"

rm -rf "$STATE/state.lock"
touch "$RELEASE"
wait "$HOLDER_PID"
unset HOLDER_PID

CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$STATE" ./build.sh gateway-status > "$TEST_ROOT/status.json"
node - "$TEST_ROOT/status.json" <<'JS'
const status = require(process.argv[2]);
if (status.state !== "idle" || status.active !== null || status.queue.length !== 0) {
  throw new Error("gateway did not release after mutex recovery");
}
JS

echo "PASS build gateway mutex timeout descendant cleanup and recovery"
