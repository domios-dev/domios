import path from 'node:path';
import { existsSync, realpathSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { inspectInstanceProcesses } from './dev-instance-inspect.mjs';
import { isDeepStrictEqual } from 'node:util';
import { LEGACY_TARGETS, SHARED_TARGET, readRecord } from './dev-named-target.mjs';

const normalize = (value, windows) => {
  if (typeof value !== 'string' || !value) return '';
  const api = windows ? path.win32 : path.posix;
  const result = api.normalize(value.replace(/^\\\\\?\\/, '')).replaceAll('\\', '/').replace(/\/$/, '');
  return windows ? result.toLowerCase() : result;
};

export function canonicalRuntimeImages(root, windows = process.platform === 'win32') {
  const suffix = windows ? '.exe' : '';
  const relative = ['domios-app', 'domios-dev-app', 'domios-server', 'codeterm',
    'domios-server/domios-server', 'cli/codeterm'].map(name => `${name}${suffix}`);
  if (windows) relative.push('conpty/x64/OpenConsole.exe', 'conpty/arm64/OpenConsole.exe');
  else relative.push('.domios-dev-runtime/bin/domios-dev-app',
    '.domios-dev-runtime/lib/codeterm/domios-server/domios-server',
    '.domios-dev-runtime/lib/codeterm/cli/codeterm',
    '.domios-dev-runtime/Domios.app/Contents/MacOS/domios-dev-app',
    '.domios-dev-runtime/Domios.app/Contents/MacOS/domios-server',
    '.domios-dev-runtime/Domios.app/Contents/Resources/cli/codeterm');
  return relative.map(name => `${root}/${name}`);
}

const namedTree = (root, windows) => /\/(?:named-dev|dev-instances\/[^/]+)$/.test(normalize(root, windows));

export function instanceTreePlan({ root, processes, listeners, port, windows = process.platform === 'win32',
  images = namedTree(root, windows) ? null : canonicalRuntimeImages(root, windows) }) {
  if (!Number.isInteger(port) || port < 1 || port > 65535 || port === 7685) throw Error('Refusing production or invalid DEV port');
  const tree = normalize(root, windows);
  if (!tree || !(windows ? path.win32 : path.posix).isAbsolute(root)) throw Error('DEV instance tree must be absolute');
  const allowed = images && new Set(images.map(value => normalize(value, windows)));
  const selected = processes.filter(row => allowed ? allowed.has(normalize(row.executable, windows))
    : normalize(row.executable, windows).startsWith(`${tree}/`));
  if (selected.some(row => !Number.isInteger(row.pid) || row.pid < 1 || !row.created)) throw Error('DEV instance process creation identity unavailable');
  const ids = new Set(selected.map(row => row.pid));
  if (listeners.some(row => row.port === 7685 && ids.has(row.pid))) throw Error('DEV image owns production port 7685; refusing cleanup');
  return selected;
}

// Shared images bound candidates; only authenticated exact-instance evidence authorizes cleanup.
export function namedInstancePlan({ root, processes, listeners, port, seeds = [], windows = process.platform === 'win32' }) {
  const candidates = instanceTreePlan({ root, processes, listeners, port, windows, images: null });
  const byPid = new Map(candidates.map(row => [row.pid, row]));
  const selected = new Set(seeds.filter(pid => byPid.has(pid)));
  for (let grew = true; grew;) {
    grew = false;
    for (const row of candidates) {
      if (!selected.has(row.pid) && selected.has(row.parent)) { selected.add(row.pid); grew = true; }
    }
  }
  return candidates.filter(row => selected.has(row.pid));
}

export function verifiedQuitProcesses(quit, { processes, listeners, data, port, windows = process.platform === 'win32' }) {
  if (port === 7685 || !data) return [];
  const ready = quit?.ready;
  const same = process => processes.some(row => row.pid === process?.pid && row.created === process.created
    && normalize(row.executable, windows) === normalize(process.executable, windows));
  if (ready?.api_address !== `127.0.0.1:${port}` || !same(ready?.process)
    || normalize(ready?.identity?.data, windows) !== normalize(data, windows)
    || normalize(ready?.identity?.config, windows) !== normalize(`${data}/config.toml`, windows)
    || !listeners.some(row => row.port === port && row.pid === ready.process.pid)
    || (quit.client && !same(quit.client.process))) return [];
  return [ready.process.pid, ...(quit.client ? [quit.client.process.pid] : [])];
}

export async function gracefulQuit({ processes, listeners, data, port }, { run = spawnSync, request = fetch } = {}) {
  if (!data || !processes.length || !existsSync(data)) return [];
  const cli = path.join(data, 'bin', `codeterm${process.platform === 'win32' ? '.exe' : ''}`);
  if (!existsSync(cli)) return [];
  let dispatched = [];
  try {
    const token = run(cli, ['system', 'api-token'], { encoding: 'utf8', windowsHide: true, timeout: 10_000,
      env: { ...Object.fromEntries(Object.entries(process.env).filter(([key]) =>
        !['CODETERM_TAB_ID', 'CODETERM_SESSION_ID', 'CODETERM_SESSION_BINDING_NONCE'].includes(key))),
      CODETERM_API_TOKEN: '', CODETERM_API_URL: '', CODETERM_DATA_DIR: data, CODETERM_API_PORT: String(port) } });
    if (token.status !== 0 || !token.stdout.trim()) return [];
    const headers = { Authorization: `Bearer ${token.stdout.trim()}` };
    const origin = `http://127.0.0.1:${port}`;
    const snapshot = await request(`${origin}/api/app/quit/snapshot`, { headers, redirect: 'error', signal: AbortSignal.timeout(5000) });
    if (!snapshot.ok) return [];
    const quit = await snapshot.json();
    const ids = verifiedQuitProcesses(quit, { processes, listeners, data: realpathSync.native(data), port });
    if (!ids.length) return [];
    dispatched = ids;
    const response = await request(`${origin}/api/app/quit`, { method: 'POST',
      headers: { ...headers, 'Content-Type': 'application/json' }, body: JSON.stringify(quit),
      redirect: 'error', signal: AbortSignal.timeout(30_000) });
    if (response.ok) {
      const reply = await response.json();
      if (reply.outcome === 'completed' && isDeepStrictEqual(reply.expected, quit)
        && reply.shutdown_completion?.operation === 'shutdown' && reply.shutdown_completion.cleanup === 'completed') {
        console.log('Authenticated DEV quit completed; waiting for pinned process exit before fallback cleanup.');
      }
    }
    return ids;
  } catch { return dispatched; }
}

// A named instance not yet moved to the shared target still runs from its legacy target, which its own record names.
export function namedInstanceRoot(checkout, data) {
  const shared = path.join(checkout, SHARED_TARGET);
  const legacy = path.join(checkout, LEGACY_TARGETS, path.basename(data).replace(/^\.domios-dev-/, ''));
  const server = data ? readRecord(path.join(data, 'dev-build-provenance.json'))?.artifacts.server.path : null;
  const windows = process.platform === 'win32';
  return server && existsSync(legacy) && normalize(server, windows).startsWith(`${normalize(realpathSync.native(legacy), windows)}/`)
    ? legacy : shared;
}

export async function stopInstanceTree(root, port, instance, data, { inspect = inspectInstanceProcesses, quit = gracefulQuit, checkout = process.cwd() } = {}) {
  checkout = realpathSync.native(checkout);
  const absolute = instance ? namedInstanceRoot(checkout, data) : path.resolve(root);
  if (!existsSync(absolute)) return;
  const physical = realpathSync.native(absolute);
  const target = path.join(checkout, 'src-tauri/target');
  const expected = instance ? [path.join(checkout, SHARED_TARGET), path.join(checkout, LEGACY_TARGETS, path.basename(absolute))]
    : [path.join(target, process.env.CARGO_BUILD_TARGET ?? '', 'debug')];
  if (!expected.some(value => normalize(physical, process.platform === 'win32') === normalize(value, process.platform === 'win32'))) {
    throw Error('DEV instance tree is aliased; refusing cleanup');
  }
  const snapshot = inspect('Inspect', instance ? { ports: [port] } : {});
  if (!instance) {
    const images = canonicalRuntimeImages(physical);
    const processes = instanceTreePlan({ root: physical, port, images, ...snapshot });
    const gracefulPids = await quit({ ...snapshot, processes, data, port });
    const result = inspect('StopTree', { root: physical, port, processes, images, gracefulPids });
    for (const row of result.stopped) console.log(`Stopped DEV pid=${row.pid} created=${row.created} image=${row.executable}`);
    return;
  }
  const candidates = instanceTreePlan({ root: physical, port, images: null, ...snapshot });
  if (!candidates.length && !snapshot.listeners.some(row => row.port === port)) return;
  const gracefulPids = await quit({ ...snapshot, processes: candidates, data, port });
  if (!gracefulPids.length) {
    if (!snapshot.listeners.some(row => row.port === port)) {
      console.log('No selected DEV API listener; refusing force cleanup without exact-instance evidence.');
      return;
    }
    throw Error('Uncertain DEV instance identity; refusing shutdown without authenticated exact-instance evidence');
  }
  const processes = namedInstancePlan({ root: physical, port, seeds: gracefulPids, ...snapshot });
  const result = inspect('StopTree', { root: physical, port, processes, images: null, gracefulPids, onlyPinned: true });
  for (const row of result.stopped) console.log(`Stopped DEV instance=${instance} pid=${row.pid} created=${row.created} image=${row.executable}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const operation = process.argv[2] === '--frontend'
    ? async () => {
      const result = inspectInstanceProcesses('StopFrontend', { repo: realpathSync.native(process.argv[3]), port: Number(process.argv[4]) });
      for (const row of result.stopped) console.log(`Stopped DEV frontend pid=${row.pid} created=${row.created} image=${row.executable}`);
    }
    : () => stopInstanceTree(process.argv[2], Number(process.argv[3]), process.argv[4], process.argv[5]);
  operation()
    .catch(error => { console.error(error.message); process.exitCode = 1; });
}
