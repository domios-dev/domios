// Caller: scripts build-gateway-mutex.test.mjs, build-gateway.sh
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { publicationGuard } from './build-gateway-guard.mjs';
import { mutexTimeoutEvidence } from './build-gateway-mutex-evidence.mjs';

const [operation, directory, pid, startToken, timeout = '30000', scope = 'production'] = process.argv.slice(2);
const mutex = path.join(directory, 'state.lock');
const guard = path.join(directory, 'state.lock.guard');
const script = fileURLToPath(new URL('./build-gateway.sh', import.meta.url)).replaceAll('\\', '/');
const deadline = Date.now() + Number(timeout);
const publisher = publicationGuard(directory);
const read = (name) => {
  try { return fs.readFileSync(path.join(mutex, name), 'utf8').trim(); }
  catch (error) { if (['ENOENT', 'EACCES', 'EPERM'].includes(error.code)) return ''; throw error; }
};
const fingerprint = () => [mutex, guard].map((name) => {
  try { const s = fs.statSync(name); return `${s.ino}:${s.mtimeMs}:${s.size}`; }
  catch (error) { if (['ENOENT', 'EACCES', 'EPERM'].includes(error.code)) return ''; throw error; }
}).join('|') + `|${read('pid')}|${read('start_token')}`;
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const alive = (owner, token) => spawnSync(bash, [
  '-c', 'source "$1"; ct_gateway_identity_alive "$2" "$3"', 'gateway', script, owner, token,
], { stdio: 'ignore' }).status === 0;

const gone = (owner, token) => spawnSync(bash, [
  '-c', 'source "$1"; ct_gateway_owner_gone "$2" "$3"', 'gateway', script, owner, token,
], { stdio: 'ignore' }).status === 0;

let pending;
const watcher = fs.watch(directory, (_, name) => pending?.(true, name?.toString() ?? ''));
let ownershipWatcher;
async function changed(previous, identityEvent = false, publicationEvent = false) {
  ownershipWatcher?.close();
  ownershipWatcher = undefined;
  try { ownershipWatcher = fs.watch(mutex, () => pending?.(true, 'state.lock')); }
  catch (error) { if (!['ENOENT', 'EACCES', 'EPERM'].includes(error.code)) throw error; }
  if (fingerprint() !== previous) return;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending = undefined;
      reject(new Error(publisher.identityUnavailable
        ? 'Gateway publication helper identity unavailable; no ownership published. Retry-safe after process lookup recovers (exit 75).'
        : 'Timed out acquiring build gateway state mutex (ownership incomplete or busy).'));
    }, Math.max(0, deadline - Date.now()));
    pending = (event = false, name = '') => {
      if (publicationEvent && event && name && name !== 'state.lock') return;
      if (!((identityEvent || publicationEvent) && event) && fingerprint() === previous) return;
      clearTimeout(timer);
      pending = undefined;
      resolve();
    };
    pending();
  });
}

let candidate;
try {
  if (operation === 'acquire') {
    candidate = fs.mkdtempSync(path.join(directory, 'state.lock.pending.'));
    fs.writeFileSync(path.join(candidate, 'pid'), `${pid}\n`);
    fs.writeFileSync(path.join(candidate, 'start_token'), `${startToken}\n`);
    fs.writeFileSync(path.join(candidate, 'job_id'), `${process.env.CT_BUILD_JOB_ID || 'unknown'}\n`);
  }
  while (true) {
    if (Date.now() >= deadline) throw new Error(publisher.identityUnavailable
      ? 'Gateway publication helper identity unavailable; no ownership published. Retry-safe after process lookup recovers (exit 75).'
      : 'Timed out acquiring build gateway state mutex.');
    let inspectedOwner = '';
    let inspectedToken = '';
    let inspectedGone = false;
    if (operation === 'acquire') {
      const observed = fingerprint();
      const owner = read('pid');
      const token = read('start_token');
      // Checking a live metadata owner must not block its release behind the publication guard.
      if (owner && token && alive(owner, token)) { await changed(observed); continue; }
      inspectedOwner = owner;
      inspectedToken = token;
      inspectedGone = Boolean(owner && token && gone(owner, token));
    }
    const publicationSnapshot = fingerprint();
    if (!publisher.acquire()) {
      if (publisher.identityUnavailable || fs.existsSync(guard)) await changed(publicationSnapshot, publisher.identityUnavailable);
      continue;
    }
    if (scope === 'fixture' && process.env.CT_GATEWAY_PUBLICATION_BARRIER) {
      fs.writeFileSync(process.env.CT_GATEWAY_PUBLICATION_READY, String(process.pid));
      fs.readSync(0, Buffer.alloc(1), 0, 1, null);
    }
    let completed = false;
    let publicationBlocked = false;
    try {
      if (operation === 'release') {
        if (read('pid') === pid && read('start_token') === startToken) fs.rmSync(mutex, { recursive: true });
        completed = true;
      } else {
        const owner = read('pid');
        const token = read('start_token');
        if (owner !== inspectedOwner || token !== inspectedToken) continue;
        // Publication and reclamation share this guard so stale observations cannot remove a new owner.
        if (inspectedGone) fs.rmSync(mutex, { recursive: true });
        if (!fs.existsSync(mutex)) {
          fs.writeFileSync(path.join(candidate, 'metadata.json'), JSON.stringify({ pid, startToken, acquiredAt: Date.now() }));
          try {
            fs.renameSync(candidate, mutex);
            candidate = undefined;
            completed = true;
          } catch (error) {
            if (!['EPERM', 'EACCES', 'EEXIST', 'ENOTEMPTY', 'EBUSY'].includes(error.code)) throw error;
            publicationBlocked = true;
          }
        }
      }
    } finally { publisher.release(); }
    if (completed) break;
    const observed = fingerprint();
    if (publicationBlocked) await changed(observed, false, true);
    else if (fs.existsSync(mutex)) await changed(observed);
  }
} catch (error) {
  console.error(error.message);
  console.error(`[build-gateway] mutex refusal caller_job=${process.env.CT_BUILD_JOB_ID || 'unknown'} evidence=${JSON.stringify(mutexTimeoutEvidence(directory))}`);
  const blockedPath = fs.existsSync(guard) ? guard : mutex;
  const quotedPath = `'${blockedPath.replaceAll("'", "'\\''")}'`;
  console.error(`If ownership is unreadable, stop gateway callers and verify no owner remains, ask the owner to resolve ${quotedPath}; do not delete unknown shared ownership`);
  process.exitCode = 75;
} finally {
  watcher.close();
  ownershipWatcher?.close();
  if (candidate) fs.rmSync(candidate, { recursive: true, force: true });
}
