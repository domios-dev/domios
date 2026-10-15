// Caller: skill build
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { readJobJson, writeJobJson } from './build-gateway-record.mjs';

const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const env = { ...process.env, PATH: process.platform === 'win32' ? `C:/Program Files/Git/usr/bin;C:/Program Files/Git/bin;${path.dirname(process.execPath)};${process.env.PATH}` : process.env.PATH };

test('readers skip missing, incomplete and inaccessible replacement observations without retry', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-record-'));
  const file = path.join(directory, 'job.json');
  try {
    assert.equal(readJobJson(file), null);
    fs.writeFileSync(file, '{');
    assert.equal(readJobJson(file), null);
    let reads = 0;
    assert.equal(readJobJson(file, () => { reads++; throw Object.assign(new Error('delete pending'), { code: 'EPERM' }); }), null);
    assert.equal(reads, 1);
    writeJobJson(file, { job_id: 'valid', state: 'queued' });
    assert.deepEqual(readJobJson(file), { job_id: 'valid', state: 'queued' });
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('failed atomic replacement preserves the previous record and removes only its temporary file', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-record-'));
  const file = path.join(directory, 'job.json');
  try {
    writeJobJson(file, { state: 'queued' });
    let renames = 0;
    assert.throws(() => writeJobJson(file, { state: 'running' }, { ...fs, renameSync() { renames++; throw Object.assign(new Error('replacement denied'), { code: 'EPERM' }); } }), /replacement denied/);
    assert.equal(renames, 1);
    assert.deepEqual(readJobJson(file), { state: 'queued' });
    assert.deepEqual(fs.readdirSync(directory), ['job.json']);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

for (const failure of ['create', 'running', 'evidence-start']) {
  test(`${failure} metadata failure aborts acquisition before compile and releases the metadata mutex`, { timeout: 30000 }, () => {
    const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-write-failure-'));
    const result = spawnSync(bash, ['-c', `
      source scripts/build-gateway.sh
      ct_repo_root() { printf '%s\\n' "$PWD"; }
      ct_gateway_set_fixture_dir "$1"
      failure="$2"
      CODETERM_BUILD_GATEWAY_MODE=queue
      CT_BUILD_JOB_ID=write-failure
      ct_gateway_state_tool() {
        if [ "$1" = "$failure" ] || { [ "$failure" = running ] && [ "$1" = update ]; }; then
          echo '[build-gateway] EPERM: fixture metadata replace failed' >&2
          return 1
        fi
        node scripts/build-gateway-state.mjs "$@"
      }
      if ct_gateway_acquire 'check-queue fixture'; then
        echo COMPILE_STARTED
        ct_gateway_finish 0
      else
        exit "$?"
      fi
    `, 'fixture', directory, failure], { cwd: root, env, encoding: 'utf8', timeout: 25000 });
    try {
      assert.equal(result.status, 74, result.stderr);
      assert.doesNotMatch(result.stdout, /COMPILE_STARTED/);
      assert.match(result.stderr, /metadata write failed before compilation/);
      assert.equal(fs.existsSync(path.join(directory, 'state.lock')), false);
      if (failure === 'create') assert.equal(fs.readdirSync(path.join(directory, 'queue')).length, 0);
    } finally { fs.rmSync(directory, { recursive: true, force: true }); }
  });
}

test('gateway status skips a partial JSON record and keeps a valid job', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-readers-'));
  try {
    fs.mkdirSync(path.join(directory, 'jobs'));
    fs.writeFileSync(path.join(directory, 'jobs', 'partial.json'), '{');
    fs.writeFileSync(path.join(directory, 'jobs', 'null.json'), 'null');
    writeJobJson(path.join(directory, 'jobs', 'valid.json'), { job_id: 'valid', state: 'completed', ended_at: '2026-10-14T18:00:00Z' });
    const result = spawnSync(process.execPath, ['scripts/build-gateway-state.mjs', 'status', '--dir', directory], { cwd: root, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    assert.deepEqual(JSON.parse(result.stdout).recent.map(j => j.job_id), ['valid']);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

for (const identity of ['readable', 'unreadable']) {
test(identity === 'readable' ? 'dead queued owner is reaped without removing the live caller ticket' : 'unreadable start identity never reaps a live queued owner', { timeout: 30000 }, () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-queued-owner-'));
  const result = spawnSync(bash, ['-c', `
    set -e
    source scripts/build-gateway.sh
    ct_gateway_set_fixture_dir "$1"
    mkdir -p "$1/jobs" "$1/queue"
    ct_gateway_mutex_acquire "$1"
    ct_gateway_capture_owner
    live_token="$CT_GATEWAY_PROCESS_START"
    for job in dead live; do
      if [ "$job" = dead ]; then pid=99999999; token=gone; else pid=$$; token="$live_token"; fi
      mkdir "$1/queue/$job"
      printf '%s\\n' "$pid" > "$1/queue/$job/pid"
      printf '%s\\n' "$token" > "$1/queue/$job/start_token"
      if [ "$job" = live ]; then
        ct_gateway_write_owner "$1/queue/$job"
      else
        printf '%s\\n' 99999999 > "$1/queue/$job/native_pid"
        printf '%s\\n' gone > "$1/queue/$job/native_start_token"
      fi
      printf '%s\\n' "$job" > "$1/queue/$job/job_id"
      ct_gateway_state_tool create --dir "$1" --job-id "$job" --job check-queue --command fixture \
        --owner "$job" --owner-pid "$pid" --owner-start-token "$token" --queue-key "$job" --queued-at fixture
    done
    if [ "$2" = unreadable ]; then
      ct_gateway_process_start_token() { return 2; }
      rm "$1/queue/live/native_start_token"
    fi
    ct_gateway_reap_stale_queue_locked "$1"
    ct_gateway_state_tool status --dir "$1"
    ct_gateway_mutex_release "$1"
  `, 'fixture', directory, identity], { cwd: root, env, encoding: 'utf8', timeout: 25000 });
  try {
    assert.equal(result.status, 0, result.stderr);
    const status = JSON.parse(result.stdout);
    assert.deepEqual(status.queue.map(j => j.job_id), ['live']);
    assert.equal(status.recent[0].job_id, 'dead');
    assert.equal(status.recent[0].outcome, 'owner_lost');
    assert.equal(fs.existsSync(path.join(directory, 'queue', 'dead')), false);
    assert.equal(fs.existsSync(path.join(directory, 'queue', 'live')), true);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

}
