// Caller: scripts/build-gateway-mutex.mjs; scripts/build-gateway-mutex-evidence.test.mjs
import fs from 'node:fs';
import path from 'node:path';

const read = filename => {
  try { return fs.readFileSync(filename, 'utf8').trim(); }
  catch { return ''; }
};

export function mutexTimeoutEvidence(directory, now = Date.now()) {
  const lock = path.join(directory, 'state.lock');
  let metadata = {};
  try { metadata = JSON.parse(read(path.join(lock, 'metadata.json'))); } catch {}
  let acquired = Number(metadata.acquiredAt);
  let ageSource = 'publication';
  if (!Number.isFinite(acquired)) {
    ageSource = 'directory-mtime';
    try { acquired = fs.statSync(lock).mtimeMs; } catch { acquired = NaN; }
  }
  let tickets = [];
  try { tickets = fs.readdirSync(path.join(directory, 'queue')).sort(); } catch {}
  const tuple = location => ({
    pid: read(path.join(location, 'pid')) || 'unknown',
    start_token: read(path.join(location, 'start_token')) || 'unknown',
    job_id: read(path.join(location, 'job_id')) || 'unknown',
  });
  return {
    owner: { ...tuple(lock), age_ms: Number.isFinite(acquired) ? Math.max(0, Math.floor(now - acquired)) : null, age_source: ageSource },
    waiters: tickets.map(ticket => ({ ticket, ...tuple(path.join(directory, 'queue', ticket)) })),
  };
}
