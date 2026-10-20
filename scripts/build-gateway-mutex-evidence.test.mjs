// Caller: skill build; build.sh script-guards
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { mutexTimeoutEvidence } from './build-gateway-mutex-evidence.mjs';

test('mutex refusal captures the owner tuple, publication age and queued waiters', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-mutex-evidence-'));
  try {
    const lock = path.join(directory, 'state.lock');
    const ticket = path.join(directory, 'queue', 'ticket-1');
    fs.mkdirSync(lock);
    fs.mkdirSync(ticket, { recursive: true });
    for (const [location, pid, token, job] of [[lock, '11', 'start-11', 'job-owner'], [ticket, '22', 'start-22', 'job-waiter']]) {
      for (const [name, value] of [['pid', pid], ['start_token', token], ['job_id', job]]) fs.writeFileSync(path.join(location, name), value);
    }
    fs.writeFileSync(path.join(lock, 'metadata.json'), JSON.stringify({ acquiredAt: 1000 }));
    assert.deepEqual(mutexTimeoutEvidence(directory, 1500), {
      owner: { pid: '11', start_token: 'start-11', job_id: 'job-owner', age_ms: 500, age_source: 'publication' },
      waiters: [{ ticket: 'ticket-1', pid: '22', start_token: 'start-22', job_id: 'job-waiter' }],
    });
    fs.unlinkSync(path.join(lock, 'start_token'));
    assert.equal(mutexTimeoutEvidence(directory, 1500).owner.start_token, 'unknown');
    assert.ok(fs.existsSync(lock), 'diagnostic reads never reclaim ownership');
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('unpublished ownership is reported as unknown without invented identity or age', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'gateway-mutex-evidence-'));
  try {
    assert.deepEqual(mutexTimeoutEvidence(directory, 1500), {
      owner: { pid: 'unknown', start_token: 'unknown', job_id: 'unknown', age_ms: null, age_source: 'directory-mtime' }, waiters: [],
    });
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});
