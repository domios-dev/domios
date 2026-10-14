// Caller: scripts/build-gateway-state.mjs; scripts/build-gateway-evidence.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import readline from 'node:readline';
import { spawnSync } from 'node:child_process';
import { serverAddress, serverIsListening } from './sccache-server-probe.mjs';

const total = counter => Object.values(counter?.counts ?? {}).reduce((sum, value) => sum + value, 0);

export function cacheSnapshot(payload, endpoint) {
  const stats = payload.stats;
  if (!stats || !Number.isFinite(stats.compile_requests)) throw new Error('missing sccache counters');
  return { requests: stats.compile_requests, hits: total(stats.cache_hits), misses: total(stats.cache_misses),
    non_cacheable: stats.requests_not_cacheable ?? 0, not_cached: stats.not_cached ?? {},
    location: payload.cache_location, endpoint };
}

export function cacheDelta(start, end, reset = false) {
  if (!start || !end || reset || start.location !== end.location || JSON.stringify(start.endpoint) !== JSON.stringify(end.endpoint)) return null;
  const keys = ['requests', 'hits', 'misses', 'non_cacheable'];
  if (keys.some(key => end[key] < start[key])) return null;
  if (Object.entries(start.not_cached).some(([key, value]) => (end.not_cached[key] ?? 0) < value)) return null;
  const delta = Object.fromEntries(keys.map(key => [key, end[key] - start[key]]));
  delta.not_cached = Object.fromEntries(Object.entries(end.not_cached).map(([key, value]) => [key, value - (start.not_cached[key] ?? 0)]).filter(([, value]) => value > 0));
  delta.hit_rate = delta.hits + delta.misses ? delta.hits / (delta.hits + delta.misses) : null;
  return delta;
}

export function fingerprintEvent(raw) {
  const line = raw.replace(/\x1b\[[0-9;]*m/g, '');
  if (!line.includes('cargo::core::compiler::fingerprint')) return null;
  const scope = line.match(/prepare_target\{(.*?)\}: cargo::core::compiler::fingerprint:/)?.[1];
  const dirty = line.match(/fingerprint (?:dirty|error) for (.+)$/)?.[1];
  const reason = line.match(/fingerprint:\s+(?:dirty|err):\s*(.+)$/)?.[1];
  if (dirty) {
    const crate = dirty.match(/^(\S+) v([^\s/]+)/);
    const mode = dirty.match(/\/(Build|Check|RunCustomBuild|Test|Doc|Doctest)\//)?.[1];
    const target = scope?.match(/target="([^"]+)"/)?.[1];
    const unit = crate ? `${crate[1]}@${crate[2]}${mode ? `/${mode}` : ''}${target ? `/${target}` : ''}` : dirty;
    return { scope: scope ?? '', unit, reason: null };
  }
  if (reason) return { scope: scope ?? '', unit: null, reason };
  return null;
}

export function nonCacheableEvent(raw) {
  const match = raw.match(/parse_arguments: CannotCache\((.*?)\): (\[.*\])/);
  if (!match) return null;
  let args;
  try { args = JSON.parse(match[2]); } catch { return null; }
  const option = key => args[args.indexOf(key) + 1];
  if (!args.includes('--crate-name')) return null;
  const name = option('--crate-name');
  const output = args.includes('--out-dir') ? option('--out-dir').replaceAll('\\', '/') : '';
  const owner = name === 'build_script_build' ? output.match(/\/build\/(.+?)-[a-f0-9]+$/)?.[1] : null;
  return { unit: owner ? `${owner}/build-script-build` : name, crate_type: args.includes('--crate-type') ? option('--crate-type') : null, reason: match[1] };
}

export function summarizeDiagnostics(fingerprintLines, sccacheLines) {
  const scopes = new Map();
  const units = new Map();
  for (const line of fingerprintLines) {
    const event = fingerprintEvent(line);
    if (!event) continue;
    if (event.unit) {
      scopes.set(event.scope, event.unit);
      const current = units.get(event.unit) ?? { unit: event.unit, count: 0, reasons: [] };
      current.count++;
      units.set(event.unit, current);
    } else if (scopes.has(event.scope)) {
      const current = units.get(scopes.get(event.scope));
      if (!current.reasons.includes(event.reason)) current.reasons.push(event.reason);
    }
  }
  return {
    rebuilt_units: [...units.values()].map(unit => ({ ...unit, reasons: unit.reasons.length ? unit.reasons : ['Cargo did not log a stale reason'] })),
    non_cacheable_units: sccacheLines.map(nonCacheableEvent).filter(Boolean),
  };
}

async function snapshot() {
  if (!await serverIsListening(serverAddress())) return { available: false, reason: 'server unavailable' };
  const result = spawnSync('sccache', ['--show-stats', '--stats-format', 'json'], { encoding: 'utf8', timeout: 5000, windowsHide: true });
  try {
    if (result.status !== 0) throw new Error('stats command failed');
    return { available: true, ...cacheSnapshot(JSON.parse(result.stdout), serverAddress()) };
  } catch { return { available: false, reason: 'stats unavailable' }; }
}

function logCursor(file) {
  try { const stat = fs.statSync(file); return { file, offset: stat.size, ino: stat.ino }; }
  catch { return { file, offset: 0, ino: null }; }
}

export async function startEvidence(record, directory, scope) {
  record.sccache = { start: scope === 'fixture' ? { available: false, reason: 'fixture' } : await snapshot(), end: null, delta: null };
  record.sccache_log_cursor = logCursor(process.env.SCCACHE_ERROR_LOG || path.join(directory, 'sccache-server.log'));
  record.fingerprint_log_path = path.join(directory, 'jobs', `${record.job_id}.fingerprint.log`);
}

async function selectedLines(file, predicate, start = 0) {
  if (!fs.existsSync(file)) return [];
  const lines = [];
  const input = fs.createReadStream(file, { start, encoding: 'utf8' });
  for await (const line of readline.createInterface({ input, crlfDelay: Infinity })) {
    if (predicate(line)) lines.push(line);
  }
  return lines;
}

export async function finishEvidence(record, directory) {
  if (!record.sccache?.start) return;
  record.sccache.end = record.sccache.start.reason === 'fixture' ? { available: false, reason: 'fixture' } : await snapshot();
  record.sccache.delta = record.sccache.start.available && record.sccache.end.available ? cacheDelta(record.sccache.start, record.sccache.end, record.sccache.reset_during_job) : null;
  const fingerprint = await selectedLines(record.fingerprint_log_path, line => line.includes('cargo::core::compiler::fingerprint'));
  const cursor = record.sccache_log_cursor;
  const current = logCursor(cursor.file);
  const offset = current.ino === cursor.ino && current.offset >= cursor.offset ? cursor.offset : 0;
  const cacheLines = await selectedLines(cursor.file, line => line.includes('parse_arguments: CannotCache('), offset);
  const cacheLog = path.join(directory, 'jobs', `${record.job_id}.sccache.log`);
  fs.writeFileSync(cacheLog, cacheLines.length ? `${cacheLines.join('\n')}\n` : '');
  record.diagnostics = { ...summarizeDiagnostics(fingerprint, cacheLines),
    sccache_log_path: cacheLog, per_unit_sccache_available: cacheLines.length > 0 };
}
