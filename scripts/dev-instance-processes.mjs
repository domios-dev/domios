import path from 'node:path';
import { existsSync, realpathSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';

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

export function instanceTreePlan({ root, processes, listeners, port, windows = process.platform === 'win32',
  images = normalize(root, windows).includes('/dev-instances/') ? null : canonicalRuntimeImages(root, windows) }) {
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

function helper(operation, input) {
  const windows = process.platform === 'win32';
  const script = fileURLToPath(new URL(windows ? './windows-dev-rebuild-stop.ps1' : './unix-dev-instance-processes.py', import.meta.url));
  const result = spawnSync(windows ? 'powershell.exe' : 'python3', windows
    ? ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', script, '-Operation', operation]
    : [script, operation], { input: JSON.stringify(input), encoding: 'utf8', windowsHide: true, timeout: 55_000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(result.stderr.trim() || 'DEV instance cleanup refused');
  return JSON.parse(result.stdout);
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

export async function stopInstanceTree(root, port, instance, data) {
  const absolute = path.resolve(root);
  if (!existsSync(absolute)) return;
  const physical = realpathSync.native(absolute);
  const target = path.join(realpathSync.native(process.cwd()), 'src-tauri/target');
  const expected = instance
    ? path.join(target, 'dev-instances', path.basename(absolute))
    : path.join(target, process.env.CARGO_BUILD_TARGET ?? '', 'debug');
  if (normalize(physical, process.platform === 'win32') !== normalize(expected, process.platform === 'win32')) {
    throw Error('DEV instance tree is aliased; refusing cleanup');
  }
  const snapshot = helper('Inspect', {});
  const images = instance ? null : canonicalRuntimeImages(physical);
  const processes = instanceTreePlan({ root: physical, port, images, ...snapshot });
  const gracefulPids = await gracefulQuit({ ...snapshot, processes, data, port });
  const result = helper('StopTree', { root: physical, port, processes, images, gracefulPids });
  for (const row of result.stopped) console.log(`Stopped DEV pid=${row.pid} created=${row.created} image=${row.executable}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const operation = process.argv[2] === '--frontend'
    ? async () => {
      const result = helper('StopFrontend', { repo: realpathSync.native(process.argv[3]), port: Number(process.argv[4]) });
      for (const row of result.stopped) console.log(`Stopped DEV frontend pid=${row.pid} created=${row.created} image=${row.executable}`);
    }
    : () => stopInstanceTree(process.argv[2], Number(process.argv[3]), process.argv[4], process.argv[5]);
  operation()
    .catch(error => { console.error(error.message); process.exitCode = 1; });
}
