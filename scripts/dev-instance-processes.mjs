import path from 'node:path';
import { existsSync, realpathSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const normalize = (value, windows) => {
  if (typeof value !== 'string' || !value) return '';
  const api = windows ? path.win32 : path.posix;
  const result = api.normalize(value.replace(/^\\\\\?\\/, '')).replaceAll('\\', '/').replace(/\/$/, '');
  return windows ? result.toLowerCase() : result;
};

export function instanceTreePlan({ root, processes, listeners, port, windows = process.platform === 'win32' }) {
  if (!Number.isInteger(port) || port < 1 || port > 65535 || port === 7685) throw Error('Refusing production or invalid DEV port');
  const tree = normalize(root, windows);
  if (!tree || !(windows ? path.win32 : path.posix).isAbsolute(root)) throw Error('DEV instance tree must be absolute');
  const selected = processes.filter(row => normalize(row.executable, windows).startsWith(`${tree}/`));
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

export function stopInstanceTree(root, port, instance) {
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
  const processes = instanceTreePlan({ root: physical, port, ...snapshot });
  const result = helper('StopTree', { root: physical, port, processes });
  for (const row of result.stopped) console.log(`Stopped DEV pid=${row.pid} created=${row.created} image=${row.executable}`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try { stopInstanceTree(process.argv[2], Number(process.argv[3]), process.argv[4]); }
  catch (error) { console.error(error.message); process.exitCode = 1; }
}
