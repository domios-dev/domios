// Caller: build.sh (named dev, dev-server, dev-prebuilt, kill); scripts/dev-instance-processes.mjs
import { existsSync, readdirSync, readFileSync, realpathSync, renameSync, statSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

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
      identifier: record.instance.identifier, data, built: statSync(file).mtimeMs }));
}

// Running means the recorded API port answers health as that recorded instance; a foreign listener is not this DEV.
export async function isRunning(instance, { request = fetch } = {}) {
  if (!Number.isInteger(instance.port) || instance.port < 1 || instance.port > 65535 || instance.port === 7685) return false;
  try {
    const response = await request(`http://127.0.0.1:${instance.port}/api/health`, { redirect: 'error', signal: AbortSignal.timeout(2000) });
    if (!response.ok) return false;
    const health = await response.json();
    return health?.instance?.name === instance.name && String(health.instance.api_port) === String(instance.port)
      && (!health.instance.identifier || health.instance.identifier === instance.identifier);
  } catch { return false; }
}

export async function runningFromTarget(target, home, options = {}) {
  const instances = instancesFromTarget(target, home);
  const running = await Promise.all(instances.map(instance => isRunning(instance, options)));
  return instances.filter((_, index) => running[index]);
}

export function stopCommand(instance) {
  return `CODETERM_DEV_INSTANCE='${instance.name}' CODETERM_DEV_API_PORT='${instance.port}' ./build.sh kill`;
}

// Only other instances refuse; a running selected instance is reported because the rebuild replaces it.
export function refusal(target, operation, running, selected = null) {
  if (!running.length) return null;
  return [`ERROR: refusing ${operation}: DEV instances started from ${target} are running.`,
    ...running.map(instance => `  - ${instance.name} (api ${instance.port}, data ${instance.data})`),
    'Stop each one from this checkout first (Windows locks a running executable), then rerun:',
    ...running.map(instance => `  ${stopCommand(instance)}`),
    ...(selected ? [`The selected instance ${selected.name} (api ${selected.port}) is also running. The rebuild replaces it`
      + ' once the others are stopped; if its launcher still owns it, stop it first with:', `  ${stopCommand(selected)}`] : [])].join('\n');
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
    const message = refusal(target, label, running.filter(instance => instance.name !== self),
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
