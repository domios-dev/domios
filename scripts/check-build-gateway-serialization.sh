#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TEST_ROOT="$ROOT/.research/build-perf/gateway-test-$$"
export CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$TEST_ROOT/state"
export CODETERM_BUILD_GATEWAY_POLL_SECS=0.05
export CODETERM_GATEWAY_FAKE_EVENTS="$TEST_ROOT/events.log"
mkdir -p "$TEST_ROOT"
: > "$CODETERM_GATEWAY_FAKE_EVENTS"

cleanup() {
  local rc="$?" pid
  touch "$TEST_ROOT/release-overlap" "$TEST_ROOT/release-failure" "$TEST_ROOT/release-cancel-blocker" \
    "$TEST_ROOT/release-running-cancel" 2>/dev/null || true
  for pid in ${TEST_PIDS:-}; do
    kill "$pid" 2>/dev/null || true
  done
  [ "$rc" -ne 0 ] || rm -rf "$TEST_ROOT"
  return "$rc"
}
trap cleanup EXIT

start_fake() {
  local id="$1" owner="$2" worktree="$3" duration="$4" result="$5"
  CODETERM_BUILD_GATEWAY_FIXTURE_DIR="$CODETERM_BUILD_GATEWAY_FIXTURE_DIR" \
    CODETERM_BUILD_GATEWAY_POLL_SECS="$CODETERM_BUILD_GATEWAY_POLL_SECS" \
    CODETERM_GATEWAY_FAKE_EVENTS="$CODETERM_GATEWAY_FAKE_EVENTS" \
    CT_BUILD_JOB_ID="$id" CODETERM_BUILD_OWNER_ID="$owner" \
    CODETERM_BUILD_WORKTREE_ID="$worktree" \
    ./build.sh gateway-fake "$id" "$duration" "$result" \
    > "$TEST_ROOT/$id.stdout.log" 2>&1 &
  STARTED_PID=$!
  TEST_PIDS="${TEST_PIDS:-} $STARTED_PID"
}

wait_for_queue() {
  local output="$1"
  for _ in $(seq 1 100); do
    ./build.sh gateway-status > "$output"
    if node -e '
      const s=require(process.argv[1]);
      process.exit(s.active && s.queue.length === 1 ? 0 : 1);
    ' "$output"; then
      return 0
    fi
    sleep 0.05
  done
  echo "gateway did not expose one running and one queued job" >&2
  cat "$output" >&2
  return 1
}

wait_for_active() {
  local job_id="$1" output="$2"
  for _ in $(seq 1 100); do
    ./build.sh gateway-status > "$output"
    if node -e '
      const s=require(process.argv[1]);
      process.exit(s.active?.job_id === process.argv[2] ? 0 : 1);
    ' "$output" "$job_id"; then
      return 0
    fi
    sleep 0.05
  done
  echo "gateway did not expose running job $job_id" >&2
  cat "$output" >&2
  return 1
}

start_fake overlap-first owner-a worktree-a "wait:$TEST_ROOT/release-overlap" success
first_pid=$STARTED_PID
wait_for_active overlap-first "$TEST_ROOT/overlap-active.json"
start_fake overlap-second owner-b worktree-b 0.5 success
second_pid=$STARTED_PID
wait_for_queue "$TEST_ROOT/overlap-status.json"

node - <<'JS' "$TEST_ROOT/overlap-status.json"
const status = require(process.argv[2]);
if (status.state !== "running") throw new Error("gateway state must be running");
if (status.active.job_id !== "overlap-first") throw new Error("wrong running owner");
if (status.active.owner !== "owner-a" || status.active.worktree !== "worktree-a") throw new Error("running identity missing");
if (!status.active.started_at || !status.active.log_path) throw new Error("running timing/log missing");
if (status.queue[0].job_id !== "overlap-second" || status.queue[0].queue_position !== 1) throw new Error("queue position missing");
if (status.queue[0].owner !== "owner-b" || status.queue[0].worktree !== "worktree-b") throw new Error("queued identity missing");
JS

touch "$TEST_ROOT/release-overlap"

wait "$first_pid"
wait "$second_pid"

node - <<'JS' "$CODETERM_GATEWAY_FAKE_EVENTS"
const fs = require("node:fs");
const rows = fs.readFileSync(process.argv[2], "utf8").trim().split("\n").map((line) => line.split(" "));
const times = Object.fromEntries(rows.map(([job, edge, time]) => [`${job}-${edge}`, BigInt(time)]));
if (times["overlap-first-end"] > times["overlap-second-start"]) throw new Error("fake jobs overlapped");
JS

start_fake expected-failure owner-c worktree-c "wait:$TEST_ROOT/release-failure" fail
failure_pid=$STARTED_PID
wait_for_active expected-failure "$TEST_ROOT/failure-active.json"
start_fake after-failure owner-d worktree-d 0.2 success
after_failure_pid=$STARTED_PID
wait_for_queue "$TEST_ROOT/failure-queue-status.json"
touch "$TEST_ROOT/release-failure"
if wait "$failure_pid"; then
  echo "failing fake job unexpectedly succeeded" >&2
  exit 1
fi
wait "$after_failure_pid"

start_fake cancel-blocker owner-e worktree-e "wait:$TEST_ROOT/release-cancel-blocker" success
blocker_pid=$STARTED_PID
wait_for_active cancel-blocker "$TEST_ROOT/cancel-blocker-active.json"
start_fake cancelled-queued owner-f worktree-f 0.2 success
cancelled_pid=$STARTED_PID
wait_for_queue "$TEST_ROOT/cancel-status.json"
./build.sh gateway-cancel cancelled-queued
if wait "$cancelled_pid"; then
  echo "cancelled queued job unexpectedly succeeded" >&2
  exit 1
fi
touch "$TEST_ROOT/release-cancel-blocker"
wait "$blocker_pid"

start_fake cancelled-running owner-h worktree-h "wait:$TEST_ROOT/release-running-cancel" success
running_cancel_pid=$STARTED_PID
wait_for_active cancelled-running "$TEST_ROOT/running-cancel-status.json"
./build.sh gateway-cancel cancelled-running
if wait "$running_cancel_pid"; then
  echo "cancelled running job unexpectedly succeeded" >&2
  exit 1
fi
start_fake after-running-cancel owner-i worktree-i 0.1 success
after_running_cancel_pid=$STARTED_PID
wait "$after_running_cancel_pid"

mkdir -p "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/lock"
printf '%s\n' "$$" > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/lock/pid"
printf '%s\n' definitely-not-this-process-start > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/lock/start_token"
printf '%s\n' stale-lock-owner > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/lock/job_id"
start_fake after-stale-lock owner-g worktree-g 0.1 success
after_stale_pid=$STARTED_PID
wait "$after_stale_pid"

stale_queue_key=9999999999999-stale-queue
mkdir -p "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/queue/$stale_queue_key"
printf '%s\n' "$$" > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/queue/$stale_queue_key/pid"
printf '%s\n' definitely-not-this-process-start > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/queue/$stale_queue_key/start_token"
printf '%s\n' stale-queue-owner > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/queue/$stale_queue_key/job_id"
node scripts/build-gateway-state.mjs create --dir "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR" \
  --job-id stale-queue-owner --job gateway-fake --command "gateway-fake stale" \
  --owner stale-owner --owner-pid "$$" --owner-start-token definitely-not-this-process-start \
  --worktree stale-worktree --source-sha stale-source --queue-key "$stale_queue_key" \
  --queued-at 2026-01-01T00:00:00Z --log-path "$TEST_ROOT/stale-queue.log"
mkdir "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/state.lock"
printf '%s\n' "$$" > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/state.lock/pid"
printf '%s\n' definitely-not-this-process-start > "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/state.lock/start_token"

./build.sh gateway-status > "$TEST_ROOT/final-status.json"
node - <<'JS' "$TEST_ROOT/final-status.json"
const status = require(process.argv[2]);
if (status.state !== "idle" || status.active !== null || status.queue.length !== 0) throw new Error("gateway did not release");
const byId = Object.fromEntries(status.recent.map((job) => [job.job_id, job]));
for (const id of ["overlap-first", "overlap-second", "after-failure", "cancel-blocker", "after-running-cancel", "after-stale-lock"]) {
  if (byId[id]?.state !== "completed" || !byId[id].ended_at || byId[id].exit_code !== 0) throw new Error(`${id} completion missing`);
}
if (byId["expected-failure"]?.state !== "failed" || byId["expected-failure"].exit_code !== 23) throw new Error("failure status missing");
if (byId["cancelled-queued"]?.state !== "cancelled" || byId["cancelled-queued"].exit_code !== 130) throw new Error("cancellation status missing");
if (byId["cancelled-running"]?.state !== "cancelled" || byId["cancelled-running"].exit_code !== 130) throw new Error("running cancellation status missing");
if (byId["stale-queue-owner"]?.state !== "failed" || byId["stale-queue-owner"].outcome !== "owner_lost") throw new Error("stale queue identity was not reaped before status");
for (const job of Object.values(byId)) {
  if (!job.owner || !job.job || !job.log_path || !job.queued_at) throw new Error(`observable fields missing for ${job.job_id}`);
}
JS

[ ! -d "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/lock" ] || { echo "gateway lock was not released" >&2; exit 1; }
[ ! -d "$CODETERM_BUILD_GATEWAY_FIXTURE_DIR/state.lock" ] || { echo "gateway state mutex was not released" >&2; exit 1; }
echo "PASS build gateway serialization, status, failure, cancellation, and release"
