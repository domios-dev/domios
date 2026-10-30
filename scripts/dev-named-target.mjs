// Caller: build.sh (named dev, dev-server, dev-prebuilt, kill); scripts/dev-instance-processes.mjs
import { existsSync, readdirSync, readFileSync, realpathSync, renameSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { inspectInstanceProcesses } from './dev-instance-inspect.mjs';

export const SHARED_TARGET = 'src-tauri/target/named-dev';
export const LEGACY_TARGETS = 'src-tauri/target/dev-instances';
const RECORD = 'dev-build-provenance.json';

const windows = process.platform === 'win32';
const physical = value => {
  const absolute = path.resolve(value);
  return existsSync(absolute) ? realpathSync.native(absolute) : absolute;
};
const normalize = value => {
  const result = value.replace(/^\\\\\?\\/, '').replaceAll('\\', '/').replace(/\/$/, '');
  return windows ? result.toLowerCase() : result;
};
const inside = (file, root) => typeof file === 'string' && normalize(physical(file)).startsWith(`${normalize(physical(root))}/`);

export function readRecord(file) {
  try {
    const record = JSON.parse(readFileSync(file, 'utf8'));
    return record?.schema === 3 && record.instance && record.artifacts?.server?.path ? record : null;
  } catch { return null; }
}

// A DEV belongs to a target when its own provenance names that target's server artifact.
export function instancesFromTarget(target, home) {
  if (!existsSync(home)) return [];
  return readdirSync(home, { withFileTypes: true })
    .filter(entry => entry.isDirectory() && entry.name.startsWith('.domios-dev-'))
    .map(entry => ({ data: path.join(home, entry.name), file: path.join(home, entry.name, RECORD) }))
    .map(row => ({ ...row, record: readRecord(row.file) }))
    .filter(row => row.record && inside(row.record.artifacts.server.path, target))
    .map(({ data, file, record }) => ({ name: record.instance.name, port: Number(record.instance.apiPort),
      identifier: record.instance.identifier, server: record.artifacts.server.path,
      retained: record.nativeClient?.snapshot?.client_process, data, built: statSync(file).mtimeMs }));
}

export async function isRunning(instance, { request = fetch, snapshot } = {}) {
  if (!snapshot) return 'unknown';
  const retained = instance.retained;
  if (retained) {
    const current = snapshot.processes.find(row => row.pid === retained.pid);
    if (current && (!current.created || !current.executable)) return 'unknown';
    if (current && current.created === retained.created && normalize(current.executable) === normalize(retained.executable)) return 'running';
  }
  if (!Number.isInteger(instance.port) || instance.port < 1 || instance.port > 65535 || instance.port === 7685) return 'unknown';
  for (const listener of snapshot.listeners.filter(row => row.port === instance.port)) {
    const server = snapshot.processes.find(row => row.pid === listener.pid);
    if (!server?.created || !server.executable) return 'unknown';
    if (normalize(physical(server.executable)) === normalize(physical(instance.server))) return 'running';
  }
  try {
    const response = await request(`http://127.0.0.1:${instance.port}/api/health`, { redirect: 'error', signal: AbortSignal.timeout(2000) });
    if (!response.ok) return snapshot.targetProcesses.length ? 'unknown' : 'stopped';
    const health = await response.json();
    if (health?.instance?.name === instance.name && String(health.instance.api_port) === String(instance.port)
      && (!health.instance.identifier || health.instance.identifier === instance.identifier)) return 'running';
  } catch {}
  return snapshot.targetProcesses.length ? 'unknown' : 'stopped';
}

export async function runningFromTarget(target, home, { inspect = inspectInstanceProcesses, ...options } = {}) {
  const instances = instancesFromTarget(target, home);
  let snapshot;
  try {
    snapshot = inspect('Inspect', { ports: instances.map(instance => instance.port) });
    if (!Array.isArray(snapshot.processes) || !Array.isArray(snapshot.listeners)) throw Error('Invalid process inspection');
    snapshot.targetProcesses = snapshot.processes.filter(row => row.executable && inside(row.executable, target));
  } catch {
    return [{ name: 'unknown', port: '?', data: target, state: 'unknown (process inspection failed)' }];
  }
  const states = await Promise.all(instances.map(instance => isRunning(instance, { ...options, snapshot })));
  const blocking = instances.map((instance, index) => ({ ...instance, state: states[index] }))
    .filter(instance => instance.state !== 'stopped');
  if (snapshot.targetProcesses.length && !blocking.length) blocking.push({ name: 'unrecorded', port: '?', data: target, state: 'running processes' });
  return blocking;
}

export function stopCommand(instance) {
  return `CODETERM_DEV_INSTANCE='${instance.name}' CODETERM_DEV_API_PORT='${instance.port}' ./build.sh kill`;
}

export function refusal(target, operation, running, selected = null) {
  if (!running.length) return null;
  return [`ERROR: refusing ${operation}: DEV instances started from ${target} are running or have unknown liveness.`,
    ...running.map(instance => `  - ${instance.name} (api ${instance.port}, data ${instance.data}) [${instance.state ?? 'running'}]`),
    'Stop each one from this checkout first (Windows locks a running executable), then rerun:',
    ...running.map(instance => `  ${stopCommand(instance)}`),
    ...(selected ? [`The selected instance ${selected.name} (api ${selected.port}) is also running or uncertain; stop it first with:`,
      `  ${stopCommand(selected)}`] : [])].join('\n');
}

// One-time move of the warm per-instance target into the shared one; never copies or deletes.
export async function adoptLegacyTarget(checkout, home, { rename = renameSync, log = console.log, ...options } = {}) {
  const target = path.join(checkout, SHARED_TARGET);
  const legacy = path.join(checkout, LEGACY_TARGETS);
  if (existsSync(target) || !existsSync(legacy)) return null;
  const candidates = readdirSync(legacy, { withFileTypes: true }).filter(entry => entry.isDirectory())
    .map(entry => path.join(legacy, entry.name));
  if (!candidates.length) return null;
  let selected = candidates[0];
  if (candidates.length > 1) {
    const built = candidates.map(directory => ({ directory,
      built: Math.max(-Infinity, ...instancesFromTarget(directory, home).map(instance => instance.built)) }))
      .filter(row => row.built > -Infinity).sort((left, right) => right.built - left.built);
    if (!built.length) {
      log(`[named-dev] not adopting: ${candidates.length} targets under ${legacy} and no DEV provenance names any of them`);
      return null;
    }
    selected = built[0].directory;
  }
  const blocking = await runningFromTarget(selected, home, options);
  if (blocking.length) throw Error(refusal(selected, `adoption of ${selected} as ${target}`, blocking));
  rename(selected, target);
  log(`[named-dev] adopted ${selected} as ${target} (renamed; ${candidates.length - 1} other target(s) left in place)`);
  return { from: selected, to: target };
}

async function main() {
  const [operation, ...args] = process.argv.slice(2);
  if (operation === 'adopt') {
    await adoptLegacyTarget(path.resolve(args[0]), path.resolve(args[1]));
  } else if (operation === 'refuse-running') {
    const [target, home, self, label] = args;
    const running = await runningFromTarget(target, path.resolve(home));
    const message = refusal(target, label, running,
      running.find(instance => instance.name === self));
    if (message) {
      console.error(message);
      process.exitCode = 2;
    }
  } else throw Error('Expected adopt or refuse-running');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => { console.error(error.message); process.exitCode = 2; });
}
