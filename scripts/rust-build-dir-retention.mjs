// Caller: build.sh build-dir-retention; scripts tests/rust-build-dir-retention.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

// NTFS keeps last-access fresh for every file Cargo or a scanner reads, so recency is write time.
export const KEEP = { units: 2, incremental: 4, build: 3 };
const UNIT = /^(.+)-([0-9a-f]{16})$/;
const SESSION = /^(.+)-([0-9a-z]+)$/;
const IDENTITY_FIELDS = ['rustc', 'features', 'target', 'profile', 'path', 'rustflags', 'config', 'compile_kind'];

// A unit's identity excludes its dependency hashes: of one identity only the newest build can be live.
export function unitIdentity(files, readJson) {
  const json = files.filter((name) => name.endsWith('.json')).sort();
  if (!json.length) return null;
  const parts = json.map((name) => {
    const data = readJson(name) ?? {};
    return [name, ...IDENTITY_FIELDS.map((field) => JSON.stringify(data[field] ?? null))].join('|');
  });
  return parts.join('||');
}

const newestFirst = (a, b) => b.mtime - a.mtime;

function groupOverflow(entries, keyOf, keep) {
  const groups = new Map();
  for (const entry of entries) {
    const key = keyOf(entry);
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(entry);
  }
  return [...groups.values()].flatMap((group) => group.sort(newestFirst).slice(keep));
}

// Build-script output dirs never join a unit hash and hold native libraries the link reads, so they age per package.
export function planRetention({ units, sessions, buildDirs }, keep = KEEP) {
  return {
    units: groupOverflow(units.filter((unit) => unit.identity), (unit) => `${unit.profile}\0${unit.name}\0${unit.identity}`, keep.units),
    sessions: groupOverflow(sessions, (entry) => `${entry.profile}\0${entry.name}`, keep.incremental),
    buildDirs: groupOverflow(buildDirs, (entry) => `${entry.profile}\0${entry.name}`, keep.build),
  };
}

const newestWrite = (dir) => {
  let newest = fs.statSync(dir).mtimeMs;
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.isFile()) newest = Math.max(newest, fs.statSync(path.join(dir, entry.name)).mtimeMs);
  }
  return newest;
};

const sizeOf = (target) => {
  const stat = fs.lstatSync(target, { throwIfNoEntry: false });
  if (!stat) return 0;
  if (!stat.isDirectory()) return stat.size;
  return fs.readdirSync(target).reduce((sum, name) => sum + sizeOf(path.join(target, name)), 0);
};

const subdirs = (dir) => (fs.existsSync(dir) ? fs.readdirSync(dir, { withFileTypes: true }).filter((e) => e.isDirectory()).map((e) => e.name) : []);

export function scanBuildDir(root) {
  const units = [];
  const sessions = [];
  const buildDirs = [];
  for (const profile of subdirs(root)) {
    const base = path.join(root, profile);
    const fingerprints = path.join(base, '.fingerprint');
    for (const dir of subdirs(fingerprints)) {
      const match = UNIT.exec(dir);
      if (!match) continue;
      const full = path.join(fingerprints, dir);
      const readJson = (name) => {
        try { return JSON.parse(fs.readFileSync(path.join(full, name), 'utf8')); } catch { return null; }
      };
      units.push({ profile, name: match[1], hash: match[2], identity: unitIdentity(fs.readdirSync(full), readJson), mtime: newestWrite(full), dir: full });
    }
    for (const dir of subdirs(path.join(base, 'incremental'))) {
      const match = SESSION.exec(dir);
      if (match) sessions.push({ profile, name: match[1], mtime: newestWrite(path.join(base, 'incremental', dir)), dir: path.join(base, 'incremental', dir) });
    }
    for (const dir of subdirs(path.join(base, 'build'))) {
      const match = UNIT.exec(dir);
      if (match) buildDirs.push({ profile, name: match[1], mtime: newestWrite(path.join(base, 'build', dir)), dir: path.join(base, 'build', dir) });
    }
  }
  return { units, sessions, buildDirs };
}

// A unit is its fingerprint plus the deps files carrying its hash; removing both can only cause a rebuild.
export function unitPaths(root, unit) {
  const deps = path.join(root, unit.profile, 'deps');
  const files = fs.existsSync(deps) ? fs.readdirSync(deps).filter((name) => name.includes(`-${unit.hash}.`) || name.endsWith(`-${unit.hash}`)) : [];
  return [unit.dir, ...files.map((name) => path.join(deps, name))];
}

export function retentionPaths(root, keep = KEEP) {
  const plan = planRetention(scanBuildDir(root), keep);
  return [
    ...plan.units.flatMap((unit) => unitPaths(root, unit)),
    ...plan.sessions.map((entry) => entry.dir),
    ...plan.buildDirs.map((entry) => entry.dir),
  ];
}

function main(argv) {
  const root = argv.find((arg) => !arg.startsWith('--'));
  if (!root) throw new Error('usage: node scripts/rust-build-dir-retention.mjs <build-dir hash> [--apply]');
  const resolved = fs.realpathSync(root);
  if (!fs.existsSync(path.join(resolved, 'CACHEDIR.TAG'))) throw new Error(`not a Cargo build directory (no CACHEDIR.TAG): ${resolved}`);
  const apply = argv.includes('--apply');
  const targets = retentionPaths(resolved);
  let bytes = 0;
  for (const target of targets) {
    if (path.relative(resolved, target).startsWith('..')) throw new Error(`refusing path outside the build directory: ${target}`);
    bytes += sizeOf(target);
    if (apply) fs.rmSync(target, { recursive: true, force: true });
  }
  console.log(`build-dir-retention: ${apply ? 'removed' : 'would remove'} paths=${targets.length} gib=${(bytes / 2 ** 30).toFixed(1)} root=${resolved}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  try {
    main(process.argv.slice(2));
  } catch (error) {
    console.error(`build-dir-retention: ${error.message}`);
    process.exit(2);
  }
}
