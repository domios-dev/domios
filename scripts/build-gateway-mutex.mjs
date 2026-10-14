// Caller: scripts build-gateway-mutex.test.mjs, build-gateway.sh
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { publicationGuard } from './build-gateway-guard.mjs';

const [operation, directory, pid, startToken, timeout = '30000', scope = 'production'] = process.argv.slice(2);
const mutex = path.join(directory, 'state.lock');
const guard = path.join(directory, 'state.lock.guard');
const script = fileURLToPath(new URL('./build-gateway.sh', import.meta.url)).replaceAll('\\', '/');
const deadline = Date.now() + Number(timeout);
const publisher = publicationGuard(directory);
const read = (name) => {
  try { return fs.readFileSync(path.join(mutex, name), 'utf8').trim(); }
  catch (error) { if (error.code === 'ENOENT') return ''; throw error; }
};
const fingerprint = () => [mutex, guard].map((name) => {
  try { const s = fs.statSync(name); return `${s.ino}:${s.mtimeMs}:${s.size}`; }
  catch (error) { if (error.code === 'ENOENT') return ''; throw error; }
}).join('|') + `|${read('pid')}|${read('start_token')}`;
const alive = (owner, token) => spawnSync('bash', [
  '-c', 'source "$1"; ct_gateway_identity_alive "$2" "$3"', 'gateway', script, owner, token,
], { stdio: 'ignore' }).status === 0;

let pending;
const watcher = fs.watch(directory, () => pending?.());
let ownershipWatcher;
async function changed(previous) {
  ownershipWatcher?.close();
  ownershipWatcher = undefined;
  try { ownershipWatcher = fs.watch(mutex, () => pending?.()); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  if (fingerprint() !== previous) return;
  await new Promise((resolve, reject) => {
    const timer = setTimeout(() => {
      pending = undefined;
      reject(new Error('Timed out acquiring build gateway state mutex (ownership incomplete or busy).'));
    }, Math.max(0, deadline - Date.now()));
    pending = () => {
      if (fingerprint() === previous) return;
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
    fs.writeFileSync(path.join(candidate, 'metadata.json'), JSON.stringify({ pid, startToken }));
  }
  while (true) {
    if (Date.now() >= deadline) throw new Error('Timed out acquiring build gateway state mutex.');
    if (!publisher.acquire()) {
      const observed = fingerprint();
      if (fs.existsSync(guard)) await changed(observed);
      continue;
    }
    if (scope === 'fixture' && process.env.CT_GATEWAY_PUBLICATION_BARRIER) {
      fs.writeFileSync(process.env.CT_GATEWAY_PUBLICATION_READY, String(process.pid));
      fs.readFileSync(process.env.CT_GATEWAY_PUBLICATION_BARRIER);
    }
    let completed = false;
    try {
      if (operation === 'release') {
        if (read('pid') === pid && read('start_token') === startToken) fs.rmSync(mutex, { recursive: true });
        completed = true;
      } else {
        const owner = read('pid');
        const token = read('start_token');
        // Publication and reclamation share this guard so stale observations cannot remove a new owner.
        if (owner && token && !alive(owner, token)) fs.rmSync(mutex, { recursive: true });
        if (!fs.existsSync(mutex)) {
          fs.renameSync(candidate, mutex);
          candidate = undefined;
          completed = true;
        }
      }
    } finally { publisher.release(); }
    if (completed) break;
    const observed = fingerprint();
    if (fs.existsSync(mutex)) await changed(observed);
  }
} catch (error) {
  console.error(error.message);
  const blockedPath = fs.existsSync(guard) ? guard : mutex;
  const quotedPath = `'${blockedPath.replaceAll("'", "'\\''")}'`;
  console.error(`If ownership is unreadable, stop gateway callers and verify no owner remains, then run: rm -rf -- ${quotedPath}`);
  process.exitCode = 75;
} finally {
  watcher.close();
  ownershipWatcher?.close();
  if (candidate) fs.rmSync(candidate, { recursive: true, force: true });
}
