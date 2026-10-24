import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { isDeepStrictEqual } from 'node:util';
import { inspect, selected } from './dev-server.mjs';

const image = value => typeof value === 'string' ? value.replaceAll('\\', '/').replace(/^\/\/\?\//, '').toLowerCase() : '';

function processTree(processes, roots) {
  const tree = new Set(roots);
  for (let previous = -1; previous !== tree.size;) {
    previous = tree.size;
    for (const row of processes) if (tree.has(row.parent)) tree.add(row.pid);
  }
  return tree;
}

function serverSubcommand(commandLine) {
  if (typeof commandLine !== 'string') return null;
  const match = commandLine.match(/^\s*(?:"[^"]+"|[^\s"]+)\s+(?:"([a-z][a-z0-9-]*)"|([a-z][a-z0-9-]*))(?=\s|$)/);
  return match?.[1] ?? match?.[2] ?? null;
}

export function rebuildStopPlan({ instance, data, port, app, record, ready, quit, processes, listeners }) {
  if (!Number.isInteger(port) || port < 1 || port > 65535 || port === 7685) throw Error('Refusing production or invalid DEV port');
  const candidates = processes.filter(row => image(row.executable) === image(app));
  const owners = [...new Set(listeners.filter(row => row.port === port).map(row => row.pid))];
  if (!owners.length && !candidates.length) return [];
  const expectedData = `.domios-dev${instance ? `-${instance}` : ''}`;
  if (record?.schema !== 3 || record.instance?.name !== instance || Number(record.instance.apiPort) !== port ||
      image(record.artifacts?.app?.path) !== image(app) ||
      path.win32.basename(data) !== expectedData || image(ready?.identity?.data) !== image(data) ||
      image(ready?.identity?.config) !== image(path.win32.join(data, 'config.toml')) ||
      ready?.api_address !== `127.0.0.1:${port}` ||
      image(ready?.process?.executable) !== image(record.artifacts?.server?.path) ||
      owners.length !== 1 || owners[0] !== ready?.process?.pid) throw Error('Uncertain DEV instance identity; refusing shutdown');
  if (!isDeepStrictEqual(quit?.ready, ready)) throw Error('Uncertain DEV quit snapshot identity');
  const native = quit.client?.process;
  if (candidates.length && (candidates.length !== 1 || !native ||
      candidates[0].pid !== native.pid || candidates[0].created !== native.created ||
      image(native.executable) !== image(app) || quit.client.connection?.server_epoch !== ready.epoch ||
      quit.client.connection?.instance_id !== ready.instance_id ||
      !isDeepStrictEqual(quit.client.connection?.identity, ready.identity))) throw Error('Uncertain DEV native client identity; refusing shutdown');
  if (native && !candidates.length) throw Error('DEV native process differs from the selected artifact');
  const roots = [ready.process, ...candidates];
  for (const root of roots) {
    const actual = processes.find(row => row.pid === root.pid);
    if (!actual || actual.created !== root.created || image(actual.executable) !== image(root.executable)) throw Error('DEV process identity changed');
  }
  const tree = processTree(processes, roots.map(row => row.pid));
  const serverTree = processTree(processes, [ready.process.pid]);
  const pinned = processes.filter(row => tree.has(row.pid));
  if (pinned.some(row => !row.executable || !/^[0-9]+$/.test(row.created))) throw Error('Uncertain DEV descendant identity');
  const rootIds = new Set(roots.map(row => row.pid));
  const instanceImages = new Set([app, record.artifacts.server.path, 'domios-app.exe'].map(value => path.win32.basename(image(value))));
  const listening = new Set(listeners.map(row => row.pid));
  if (pinned.some(row => {
    if (rootIds.has(row.pid) || !instanceImages.has(path.win32.basename(image(row.executable)))) return false;
    const command = serverSubcommand(row.commandLine);
    const helper = serverTree.has(row.pid) && image(row.executable) === image(record.artifacts.server.path) &&
      command && command !== 'serve' && !listening.has(row.pid);
    return !helper;
  })) {
    throw Error('DEV process tree contains another instance; refusing shutdown');
  }
  if (listeners.some(row => tree.has(row.pid) && (row.port === 7685 ||
      (rootIds.has(row.pid) && ![port, Number(record.instance.cdpPort)].includes(row.port))))) {
    throw Error('DEV process tree owns a production or foreign root listener; refusing shutdown');
  }
  return pinned;
}

function windowsHelper(operation, input) {
  const result = spawnSync('powershell.exe', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
    fileURLToPath(new URL('./windows-dev-rebuild-stop.ps1', import.meta.url)), '-Operation', operation],
  { input: JSON.stringify(input), encoding: 'utf8', windowsHide: true, timeout: 55_000 });
  if (result.error) throw result.error;
  if (result.status !== 0) throw Error(result.stderr.trim() || 'DEV shutdown helper refused');
  return JSON.parse(result.stdout);
}

async function main() {
  const [recordFile, data, portText, instance, appText] = process.argv.slice(2);
  const app = existsSync(appText) ? realpathSync.native(appText) : path.resolve(appText);
  const port = Number(portText);
  const snapshot = windowsHelper('Inspect', {});
  if (!snapshot.listeners.some(row => row.port === port) && !snapshot.processes.some(row => image(row.executable) === image(app))) {
    rebuildStopPlan({ instance, data, port, app, ...snapshot });
    return;
  }
  const record = existsSync(recordFile) ? JSON.parse(readFileSync(recordFile, 'utf8')) : null;
  const target = selected(recordFile, data, port);
  const ready = await inspect(target);
  const token = spawnSync(record.artifacts.cli.path, ['system', 'api-token'], {
    encoding: 'utf8', windowsHide: true, timeout: 10_000,
    env: { ...process.env, CODETERM_API_TOKEN: '', CODETERM_API_URL: '', CODETERM_DATA_DIR: data, CODETERM_API_PORT: String(port) },
  });
  if (token.status !== 0 || !token.stdout.trim()) throw Error('DEV shutdown credential unavailable');
  const response = await fetch(`http://127.0.0.1:${port}/api/app/quit/snapshot`, {
    headers: { Authorization: `Bearer ${token.stdout.trim()}` }, redirect: 'error', signal: AbortSignal.timeout(10_000),
  });
  if (!response.ok) throw Error('DEV quit identity unavailable; refusing shutdown');
  const quit = await response.json();
  const processes = rebuildStopPlan({ instance, data, port, app, record, ready, quit, ...snapshot });
  windowsHelper('Stop', { processes, quit, port,
    allowedListeners: snapshot.listeners.filter(row => processes.some(process => process.pid === row.pid)),
    token: token.stdout.trim() });
  console.log(`DEV instance '${instance || 'default'}' pinned process exit confirmed before artifact replacement.`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 1; });
}
