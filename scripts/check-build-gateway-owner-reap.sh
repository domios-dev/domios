#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TEST_ROOT="$ROOT/.research/build-gateway-owner-reap-$$"
STATE="$TEST_ROOT/state"
mkdir -p "$STATE/jobs" "$STATE/queue"

HOLDER_PID=""
KILLED_PID=""
FAILURES=0
ACQUIRE_RC=0
RELEASE_FIFO="$TEST_ROOT/holder.release.fifo"

cleanup() {
  local rc="$?"
  [ -z "$HOLDER_PID" ] || kill -KILL "$HOLDER_PID" 2>/dev/null || true
  [ -z "$KILLED_PID" ] || kill -KILL "$KILLED_PID" 2>/dev/null || true
  [ "$rc" -ne 0 ] || rm -rf "$TEST_ROOT"
  return "$rc"
}
trap cleanup EXIT

ct_repo_root() { git rev-parse --show-toplevel 2>/dev/null || pwd; }
source scripts/build-gateway.sh
ct_gateway_set_fixture_dir "$STATE"

# abort() is for harness faults that make the remaining sections meaningless.
# note_fail() records a failed assertion and keeps going, so one leg of a
# red/green reports every section rather than stopping at the first difference.
abort() { echo "ABORT: $*" >&2; exit 2; }
note_fail() { echo "FAIL: $*" >&2; FAILURES=$((FAILURES + 1)); }

record_field() {
  node -e 'const r=require(process.argv[1]);const v=r[process.argv[2]];process.stdout.write(v===null||v===undefined?"":String(v))' \
    "$STATE/jobs/$1.json" "$2"
}

# Every local is assigned in its own statement: bash expands all words of a
# `local` before any of its assignments, so a cross-reference reads outer scope.
seed_owned_job() {
  local job="$1"
  local pid="$2"
  local token="$3"
  local queue_key="0000000000000-0000000000-$job"
  ct_gateway_state_tool create --dir "$STATE" --job-id "$job" --job probe \
    --command "probe $job" --owner reap-fixture --owner-pid "$pid" \
    --owner-start-token "$token" --owner-process-command "bash ./build.sh probe" \
    --worktree "$ROOT" --source-sha fixture --queue-key "$queue_key" \
    --queued-at "$(ct_gateway_now)" --log-path "$STATE/jobs/$job.log"
  CT_BUILD_JOB_ID="$job" ct_gateway_update_job running "" "" "$(ct_gateway_now)" ""
  mkdir -p "$STATE/lock"
  printf '%s\n' "$pid" > "$STATE/lock/pid"
  printf '%s\n' "$token" > "$STATE/lock/start_token"
  printf '%s\n' "$job" > "$STATE/lock/job_id"
  printf '%s\n' "$queue_key" > "$STATE/lock/queue_key"
  printf '%s\n' "probe $job" > "$STATE/lock/command"
  printf '%s\n' "$ROOT" > "$STATE/lock/worktree"
  printf '%s\n' fixture > "$STATE/lock/source_sha"
}

start_token_or_empty() {
  ct_gateway_process_start_token "$1" 2>/dev/null || true
}

ACQUIRE_BODY='
  set -e
  ct_repo_root() { git rev-parse --show-toplevel 2>/dev/null || pwd; }
  source scripts/build-gateway.sh
  ct_gateway_acquire "reap-probe"
  echo "ACQUIRED job=$CT_BUILD_JOB_ID"
'

# Reports through ACQUIRE_RC and always returns 0. Returning the child's status
# would abort the caller under `set -e`, and toggling errexit inside a function
# changes it for the whole shell, not just the call.
run_acquire() {
  local out="$1"
  ACQUIRE_RC=0
  env CT_BUILD_GATEWAY_SCOPE=fixture CT_BUILD_GATEWAY_FIXTURE_DIR="$STATE" \
      CODETERM_BUILD_GATEWAY_MODE=fail-fast CODETERM_BUILD_OWNER_ID=reap-probe \
      bash -c "$ACQUIRE_BODY" > "$out" 2>&1 || ACQUIRE_RC=$?
  return 0
}

# Bounded, and never asks a `ps` whose no-match exit status varies by platform.
dead_pid_outside_every_table() {
  local candidate=4194303
  while [ "$candidate" -gt 1 ]; do
    if [ ! -r "/proc/$candidate/stat" ] && ! kill -0 "$candidate" 2>/dev/null; then
      printf '%s\n' "$candidate"
      return 0
    fi
    candidate=$((candidate - 1))
  done
  return 1
}

echo "== 0. fixture context"
# Probed here, not asked of the code under test: the pre-fix gateway has no
# namespace helper, and a test that calls one cannot run against it.
if [ -r /proc/self/stat ]; then
  NAMESPACE=proc
else
  NAMESPACE=native
fi
echo "   pid namespace: $NAMESPACE"
# The blob of the file under test, not HEAD: a red/green leg swaps that one file,
# so HEAD would label both legs identically.
echo "   build-gateway.sh blob: $(git hash-object scripts/build-gateway.sh 2>/dev/null || echo unknown)"
if grep -q '^ct_gateway_pid_namespace()' scripts/build-gateway.sh; then
  echo "   gateway source carries the single-namespace rule: yes"
else
  echo "   gateway source carries the single-namespace rule: no"
fi
echo "   state dir: $STATE"

echo "== 1. an acquire that finds a dead owner reaps it and succeeds"
DEAD_PID="$(dead_pid_outside_every_table || true)"
[ -n "$DEAD_PID" ] || abort "could not find a pid absent from every process table"
echo "   dead owner pid: $DEAD_PID"
seed_owned_job dead-owner "$DEAD_PID" 1234567890
run_acquire "$TEST_ROOT/acquire-dead.log"
if [ "$ACQUIRE_RC" -ne 0 ]; then
  sed 's/^/   | /' "$TEST_ROOT/acquire-dead.log" >&2
  note_fail "acquire reported a dead owner as the blocker instead of reaping it (exit $ACQUIRE_RC)"
else
  grep -q ACQUIRED "$TEST_ROOT/acquire-dead.log" || note_fail "acquire did not take the gateway"
fi
[ "$(record_field dead-owner state)" = failed ] || note_fail "reaped job did not reach a terminal state"
[ "$(record_field dead-owner outcome)" = owner_lost ] || note_fail "reaped job outcome is not owner_lost"
[ -n "$(record_field dead-owner ended_at)" ] || note_fail "reaped job kept a null ended_at"
[ ! -d "$STATE/lock" ] || note_fail "the gateway lock survived the reap"

echo "== 2. a live owner still blocks the acquire"
mkfifo "$TEST_ROOT/holder.ready.fifo" "$RELEASE_FIFO"
bash -c 'echo ready > "$1"; read -r _ < "$2"' _ "$TEST_ROOT/holder.ready.fifo" "$RELEASE_FIFO" &
HOLDER_PID=$!
read -r _ < "$TEST_ROOT/holder.ready.fifo"
HOLDER_TOKEN="$(start_token_or_empty "$HOLDER_PID")"
[ -n "$HOLDER_TOKEN" ] || abort "no start token for live holder pid $HOLDER_PID"
echo "   live owner pid: $HOLDER_PID"
seed_owned_job live-owner "$HOLDER_PID" "$HOLDER_TOKEN"
run_acquire "$TEST_ROOT/acquire-live.log"
[ "$ACQUIRE_RC" -eq 75 ] || note_fail "a live owner did not block the acquire (exit $ACQUIRE_RC)"
grep -q "Build gateway busy: job=live-owner" "$TEST_ROOT/acquire-live.log" ||
  note_fail "the live owner was not reported as the blocker"
[ "$(record_field live-owner state)" = running ] || note_fail "a live owner's record was reaped"
[ -d "$STATE/lock" ] || note_fail "a live owner's lock was released"

echo "== 3. liveness is decided on one pid namespace"
TOKEN_A="$(start_token_or_empty "$HOLDER_PID")"
TOKEN_B="$(start_token_or_empty "$HOLDER_PID")"
[ -n "$TOKEN_A" ] || note_fail "the writer path produced no token for a live process"
[ "$TOKEN_A" = "$TOKEN_B" ] || note_fail "the token a start writes is not the token a later check reads"
ct_gateway_identity_alive "$HOLDER_PID" "$TOKEN_A" || note_fail "a live owner's own token did not compare alive"

PS_LIST='Get-Process | ForEach-Object { try { Write-Output "$($_.Id) $($_.StartTime.ToUniversalTime().Ticks)" } catch { } }'
PS_ONE='$p=Get-Process -Id PID_PLACEHOLDER -ErrorAction SilentlyContinue; if ($p) { [Console]::Write($p.StartTime.ToUniversalTime().Ticks) } else { exit 1 }'
windows_ticks_or_empty() {
  local script="${PS_ONE//PID_PLACEHOLDER/$1}"
  powershell.exe -NoProfile -Command "$script" 2>/dev/null | tr -d '\r' || true
}

FOREIGN_PID=""
FOREIGN_TOKEN=""
if [ "$NAMESPACE" = proc ] && command -v powershell.exe >/dev/null 2>&1; then
  # One PowerShell call for the whole table, then read from a file so the loop
  # body is not a subshell and its assignments survive.
  powershell.exe -NoProfile -Command "$PS_LIST" 2>/dev/null | tr -d '\r' \
    > "$TEST_ROOT/windows-processes.txt" || true
  SCANNED=0
  while read -r wpid wticks; do
    case "$wpid" in ''|*[!0-9]*) continue ;; esac
    [ -n "$wticks" ] || continue
    [ "$wpid" -gt 4 ] || continue
    [ -r "/proc/$wpid/stat" ] && continue
    SCANNED=$((SCANNED + 1))
    CONFIRM="$(windows_ticks_or_empty "$wpid")"
    [ "$CONFIRM" = "$wticks" ] || continue
    FOREIGN_PID="$wpid"
    FOREIGN_TOKEN="$wticks"
    break
  done < "$TEST_ROOT/windows-processes.txt"
  # A silent skip here would let this section pass by never running, which is the
  # failure mode it exists to catch. On a two-namespace host, finding nothing is a
  # broken construction, not an absent case.
  [ -n "$FOREIGN_PID" ] ||
    abort "scanned $SCANNED Windows pids absent from /proc and constructed no namespace disagreement"
fi

if [ -n "$FOREIGN_PID" ]; then
  echo "   namespace disagreement: $FOREIGN_PID is a live Windows pid and no /proc pid"
  if ct_gateway_identity_alive "$FOREIGN_PID" "$FOREIGN_TOKEN"; then
    note_fail "liveness crossed pid namespaces: pid $FOREIGN_PID was judged alive from a Windows process token"
  fi
  [ -z "$(start_token_or_empty "$FOREIGN_PID")" ] ||
    note_fail "a start token for pid $FOREIGN_PID resolved outside the /proc namespace"
  echo "   the same disagreement must not hold the machine: seed it as the running owner"
  rm -rf "$STATE/lock"
  seed_owned_job foreign-namespace-owner "$FOREIGN_PID" "$FOREIGN_TOKEN"
  run_acquire "$TEST_ROOT/acquire-foreign.log"
  if [ "$ACQUIRE_RC" -ne 0 ]; then
    sed 's/^/   | /' "$TEST_ROOT/acquire-foreign.log" >&2
    note_fail "an owner dead in the gateway's own pid namespace was reported as the blocker (exit $ACQUIRE_RC)"
  fi
  [ "$(record_field foreign-namespace-owner outcome)" = owner_lost ] ||
    note_fail "the cross-namespace owner never reached owner_lost"
  echo "   and the namespace rule must not reap a live owner"
  rm -rf "$STATE/lock"
  seed_owned_job live-owner-again "$HOLDER_PID" "$HOLDER_TOKEN"
  run_acquire "$TEST_ROOT/acquire-live-again.log"
  [ "$ACQUIRE_RC" -eq 75 ] || note_fail "the namespace rule reaped a live owner (exit $ACQUIRE_RC)"
else
  echo "   host exposes one pid namespace; no cross-namespace case to construct"
fi

echo "== 4. a trap-less exit still reaches a terminal state that says owner-vanished"
echo go > "$RELEASE_FIFO"
wait "$HOLDER_PID" 2>/dev/null || true
HOLDER_PID=""
rm -rf "$STATE/lock"
mkfifo "$TEST_ROOT/killed.ready.fifo" "$TEST_ROOT/killed.hold.fifo"
KILLED_BODY='
  set -e
  ct_repo_root() { git rev-parse --show-toplevel 2>/dev/null || pwd; }
  source scripts/build-gateway.sh
  ct_gateway_acquire "killed-probe"
  printf "Build gateway job %s exit: 0\n" "$CT_BUILD_JOB_ID" >> "$(ct_gateway_log_path)"
  printf "%s\n" "$CT_BUILD_JOB_ID" > "$1"
  read -r _ < "$2"
'
env CT_BUILD_GATEWAY_SCOPE=fixture CT_BUILD_GATEWAY_FIXTURE_DIR="$STATE" \
    CODETERM_BUILD_GATEWAY_MODE=fail-fast CODETERM_BUILD_OWNER_ID=killed-owner \
    bash -c "$KILLED_BODY" _ "$TEST_ROOT/killed.ready.fifo" "$TEST_ROOT/killed.hold.fifo" \
    > "$TEST_ROOT/killed.log" 2>&1 &
KILLED_PID=$!
read -r KILLED_JOB < "$TEST_ROOT/killed.ready.fifo"
[ -n "$KILLED_JOB" ] || abort "the killed owner never reported its job id"
echo "   killed owner pid: $KILLED_PID job: $KILLED_JOB"
kill -KILL "$KILLED_PID"
wait "$KILLED_PID" 2>/dev/null || true
KILLED_PID=""
[ "$(record_field "$KILLED_JOB" state)" = running ] ||
  abort "SIGKILL ran the EXIT trap after all; the fixture would prove nothing"
run_acquire "$TEST_ROOT/acquire-killed.log"
if [ "$ACQUIRE_RC" -ne 0 ]; then
  sed 's/^/   | /' "$TEST_ROOT/acquire-killed.log" >&2
  note_fail "an owner killed without its EXIT trap kept the gateway (exit $ACQUIRE_RC)"
fi
[ "$(record_field "$KILLED_JOB" state)" = failed ] || note_fail "the killed job did not reach a terminal state"
[ "$(record_field "$KILLED_JOB" outcome)" = owner_lost ] ||
  note_fail "the killed job's outcome does not say owner-vanished"
[ "$(record_field "$KILLED_JOB" command_exit_code)" = 0 ] ||
  note_fail "the killed job's already-reported command exit was lost"
grep -q "vanished after its command reported exit 0" "$TEST_ROOT/acquire-killed.log" ||
  note_fail "the unrecorded success was not surfaced"

CANCEL_JOB=fixture-cancelled
CANCEL_KEY="9999999999999-0000000000-$CANCEL_JOB"
SELF_TOKEN="$(start_token_or_empty "$$")"
[ -n "$SELF_TOKEN" ] || abort "no start token for this shell"
ct_gateway_state_tool create --dir "$STATE" --job-id "$CANCEL_JOB" --job probe \
  --command "probe cancel" --owner reap-fixture --owner-pid "$$" \
  --owner-start-token "$SELF_TOKEN" --owner-process-command "bash ./build.sh probe" \
  --worktree "$ROOT" --source-sha fixture --queue-key "$CANCEL_KEY" \
  --queued-at "$(ct_gateway_now)" --log-path "$STATE/jobs/$CANCEL_JOB.log"
mkdir -p "$STATE/queue/$CANCEL_KEY"
printf '%s\n' "$$" > "$STATE/queue/$CANCEL_KEY/pid"
printf '%s\n' "$SELF_TOKEN" > "$STATE/queue/$CANCEL_KEY/start_token"
printf '%s\n' "$CANCEL_JOB" > "$STATE/queue/$CANCEL_KEY/job_id"
ct_gateway_cancel "$CANCEL_JOB" || note_fail "cancelling a queued job failed"
[ "$(record_field "$CANCEL_JOB" outcome)" = cancelled ] ||
  note_fail "a human cancellation did not record cancelled"
[ "$(record_field "$KILLED_JOB" outcome)" != "$(record_field "$CANCEL_JOB" outcome)" ] ||
  note_fail "owner-vanished is indistinguishable from cancelled-by-a-human"

echo "== 5. nothing in the reap path is triggered by elapsed time"
REAP_SOURCE="$(sed -n '/^ct_gateway_record_owner_lost()/,/^}/p;/^ct_gateway_reap_stale_owner_locked()/,/^}/p;/^ct_gateway_reap_stale_queue_locked()/,/^}/p;/^ct_gateway_identity_alive()/,/^}/p' scripts/build-gateway.sh)"
[ -n "$REAP_SOURCE" ] || abort "could not read the reap path"
# Only symbols that exist in every revision, so this section is a stable
# invariant on both legs of a red/green rather than a proxy for the fix.
for symbol in ct_gateway_reap_stale_owner_locked ct_gateway_reap_stale_queue_locked \
  ct_gateway_identity_alive; do
  # Herestrings, not pipes: under `pipefail` a `grep -q` that exits on its first
  # match can SIGPIPE the writer and turn a match into a non-zero pipeline.
  grep -q "^$symbol()" <<< "$REAP_SOURCE" || abort "reap path extraction missed $symbol"
done
if grep -Eq 'sleep|date \+%s|elapsed|deadline|timeout' <<< "$REAP_SOURCE"; then
  note_fail "the reap path consults elapsed time; the trigger must be the owning process"
fi

if [ "$FAILURES" -ne 0 ]; then
  echo "FAILED: $FAILURES assertion(s) across sections 1-5" >&2
  exit 1
fi
echo "PASS build gateway owner reap, live-owner blocking, single pid namespace, trap-less terminal state"
