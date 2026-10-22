// Caller: scripts cache-policy.sh, cache-retention.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { collectCargoBuilds } from './cargo-build-gc.mjs';
import { trimGatewayCaches } from './gateway-cache-gc.mjs';
import { trimLaneTargets } from './lane-target-gc.mjs';
import { collectCheckWorktrees } from './check-worktree-paths.mjs';

const DAY = 86400000;
const markerName = '.codeterm-cache.json';
const log = (message) => console.log(`[cache-policy] ${message}`);

function noLinks(value) {
  if (!value || !path.isAbsolute(value)) throw new Error('unsafe empty or relative path');
  const absolute = path.resolve(value);
  let cursor = absolute;
  while (true) {
    if (fs.lstatSync(cursor).isSymbolicLink()) throw new Error(`unsafe link/junction: ${cursor}`);
    const parent = path.dirname(cursor);
    if (parent === cursor) break;
    cursor = parent;
  }
  return absolute;
}

export function physical(value) {
  const absolute = noLinks(value);
  const resolved = execFileSync(process.env.CT_CACHE_BASH || 'bash', ['-c', 'cd -- "$1" && pwd -P', 'cache-policy', absolute],
    { encoding: 'utf8' }).trim();
  const native = process.platform === 'win32'
    ? execFileSync('cygpath', ['-w', resolved], { encoding: 'utf8' }).trim() : resolved;
  if (path.resolve(native) !== fs.realpathSync(absolute)) throw new Error(`unsafe resolved path: ${value}`);
  return fs.realpathSync(absolute);
}

export function candidate(root, value) {
  const base = physical(root);
  if (!value || path.resolve(value) === path.parse(path.resolve(value)).root) throw new Error('unsafe root/empty path');
  const resolved = physical(value);
  if (path.dirname(resolved) !== base || resolved === base) throw new Error(`unsafe non-child: ${value}`);
  return resolved;
}

function readJson(file) {
  try {
    if (fs.lstatSync(file).isSymbolicLink()) return null;
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch { return null; }
}

function alive(pid) {
  if (!Number.isSafeInteger(pid) || pid <= 0) return false;
  try { process.kill(pid, 0); return true; } catch (error) { return error.code === 'EPERM'; }
}

function bytes(dir) {
  let size = 0;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const name = path.join(dir, entry.name);
    const stat = fs.lstatSync(name);
    if (stat.isSymbolicLink()) throw new Error(`unsafe nested link/junction: ${name}`);
    size += stat.isDirectory() ? bytes(name) : stat.size;
  }
  return size;
}

function removeTree(dir) {
  noLinks(dir);
  for (const entry of fs.readdirSync(dir)) {
    const name = path.join(dir, entry);
    const stat = fs.lstatSync(name);
    if (stat.isSymbolicLink()) throw new Error(`unsafe nested link/junction: ${name}`);
    if (stat.isDirectory()) removeTree(name);
    else fs.unlinkSync(name);
  }
  fs.rmdirSync(dir);
}

function writeJson(file, data) {
  if (fs.existsSync(file) && fs.lstatSync(file).isSymbolicLink()) throw new Error(`unsafe metadata link: ${file}`);
  const temp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(temp, JSON.stringify(data), { flag: 'wx' });
  fs.renameSync(temp, file);
}

export function mark(gateway, checkout, target, now = Date.now(), owner = process.ppid, home = gateway) {
  physical(gateway);
  const root = path.join(home, 'targets', 'exceptional');
  const dir = candidate(root, target);
  const repo = physical(checkout);
  const prior = readJson(path.join(dir, markerName));
  const owners = (Array.isArray(prior?.owners) ? prior.owners : []).filter(alive);
  writeJson(path.join(dir, markerName), { checkout: repo, lastUsed: now, sizeBytes: bytes(dir),
    owners: [...new Set([...owners, owner])] });
}

export function retentionDue(gateway, now) {
  const last = readJson(path.join(gateway, 'cache-retention.json'))?.lastRun;
  return !(Number.isFinite(last) && now - last >= 0 && now - last < 3600000);
}

export function retain(gateway, checkout, { dryRun = false, automatic = false, now = Date.now(),
  capBytes = Number(process.env.CODETERM_EXCEPTIONAL_CACHE_GB || 40) * 1024 ** 3,
  maxAgeDays = Number(process.env.CODETERM_CACHE_MAX_AGE_DAYS || 3),
  sweep = true, runner = spawnSync, home = gateway } = {}) {
  if (!Number.isFinite(capBytes) || capBytes < 0) throw new Error('invalid CODETERM_EXCEPTIONAL_CACHE_GB');
  if (!Number.isSafeInteger(maxAgeDays) || maxAgeDays < 1) throw new Error('invalid CODETERM_CACHE_MAX_AGE_DAYS');
  gateway = physical(gateway);
  checkout = physical(checkout);
  const stamp = path.join(gateway, 'cache-retention.json');
  if (automatic && !retentionDue(gateway, now)) return 0;
  const root = path.join(home, 'targets', 'exceptional');
  const records = [];
  const buildingCheckouts = new Set();
  if (fs.existsSync(root)) {
    physical(root);
    for (const name of fs.readdirSync(root)) {
      try {
        const dir = candidate(root, path.join(root, name));
        const marker = readJson(path.join(dir, markerName));
        const valid = marker && typeof marker.checkout === 'string' && path.isAbsolute(marker.checkout)
          && Number.isFinite(marker.lastUsed) && Array.isArray(marker.owners);
        if (valid && marker.owners.some(alive)) buildingCheckouts.add(marker.checkout);
        const used = valid ? marker.lastUsed : fs.statSync(dir).mtimeMs;
        const protectedDir = (valid && (marker.checkout === checkout || marker.owners.some(alive)))
          || (!valid && now - used <= 2 * DAY);
        records.push({ dir, size: bytes(dir), used, protectedDir,
          checkout: valid ? marker.checkout : null,
          stale: valid ? !fs.existsSync(marker.checkout) || now - used > maxAgeDays * DAY : now - used > 2 * DAY });
      } catch (error) { log(`refused ${name}: ${error.message}`); }
    }
  }
  for (const record of records) {
    if (record.checkout && buildingCheckouts.has(record.checkout)) record.protectedDir = true;
  }
  let total = records.reduce((sum, record) => sum + record.size, 0);
  let reclaimed = 0;
  for (const record of records.sort((a, b) => Number(b.stale) - Number(a.stale)
    || a.used - b.used || a.dir.localeCompare(b.dir))) {
    if (record.protectedDir || (!record.stale && total <= capBytes)) continue;
    const dir = candidate(root, record.dir);
    log(`${dryRun ? 'would reclaim' : 'reclaim'} ${record.size} bytes: ${dir}`);
    if (!dryRun) { bytes(dir); removeTree(dir); }
    total -= record.size;
    reclaimed += record.size;
  }
  if (total > capBytes) log(`cap exceeded by protected targets: ${total} bytes retained`);
  if (sweep) {
    reclaimed += trimGatewayCaches(gateway, { dryRun, now, maxAgeDays, runner, log });
    const common = runner('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], { cwd: checkout, encoding: 'utf8' });
    if (common.status === 0 && typeof common.stdout === 'string' && path.isAbsolute(common.stdout.trim())) {
      reclaimed += trimLaneTargets(path.dirname(common.stdout.trim()), { checkout, dryRun, maxAgeDays, now, runner, log });
    } else log('lane target trim refused: canonical checkout unavailable');
  }
  if (!dryRun) writeJson(stamp, { lastRun: now });
  log(`${dryRun ? 'would reclaim' : 'reclaimed'} ${reclaimed} bytes total`);
  return reclaimed;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.env.CT_CACHE_RETENTION_LOCK_HELD !== '1') throw new Error('use build.sh gc; cache retention mutex is required');
    const [operation, gateway, checkout, target, home = gateway] = process.argv.slice(2);
    if (operation === 'mark') mark(gateway, checkout, target, Date.now(), process.ppid, home);
    else if (operation === 'gc' || operation === 'auto') {
      if (operation === 'gc') collectCheckWorktrees(checkout, gateway, { dryRun: target === '--dry-run', log });
      let collected = 0;
      if (process.env.CT_BUILD_GATEWAY_SCOPE !== 'fixture' && (operation === 'gc' || retentionDue(gateway, Date.now()))) {
        try {
          collected = collectCargoBuilds(checkout, { dryRun: target === '--dry-run', record: path.join(gateway, 'stable-build-dirs.json') });
        } catch (error) {
          if (operation === 'gc') throw error;
          log(`orphan build collection refused: ${error.message}; build continues`);
        }
      }
      const retained = retain(gateway, checkout, { automatic: operation === 'auto', dryRun: target === '--dry-run',
        sweep: process.env.CT_BUILD_GATEWAY_SCOPE !== 'fixture', home });
      log(`${target === '--dry-run' ? 'would free' : 'freed'} ${collected + retained} bytes across all caches`);
    }
    else throw new Error('unknown cache retention operation');
  } catch (error) { console.error(`[cache-policy] ${error.message}`); process.exitCode = 2; }
}
