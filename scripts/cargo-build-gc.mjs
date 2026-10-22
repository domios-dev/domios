// Caller: scripts/cache-retention.mjs, scripts/cargo-build-gc.test.mjs
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { retentionPaths } from './rust-build-dir-retention.mjs';
import { orphanLanePaths } from './orphan-lane-units.mjs';
import { laneBuildDirectory } from './lane-build-dir.mjs';

const log = (value) => console.log(`[cargo-build-gc] ${value}`);
const key = (value) => process.platform === 'win32' ? path.resolve(value).toLowerCase() : path.resolve(value);

function command(program, args, cwd, runner) {
  const result = runner(program, args, { cwd, encoding: 'utf8', maxBuffer: 64 * 1024 ** 2 });
  if (result.error || result.status !== 0) throw new Error(`${program} ${args.join(' ')}: ${result.error?.message || result.stderr}`);
  return result.stdout;
}

export function worktrees(repo, runner = spawnSync) {
  return command('git', ['worktree', 'list', '--porcelain', '-z'], repo, runner)
    .split('\0').filter((line) => line.startsWith('worktree ')).map((line) => line.slice(9));
}

export function mapCheckouts(checkouts, { runner = spawnSync, manifests, indexCheckout = checkout => checkout } = {}) {
  const mappings = [];
  for (const checkout of [...new Set(checkouts)]) {
    if (!fs.existsSync(checkout)) continue;
    let files;
    try {
      files = manifests ? manifests(checkout) : command('git', ['ls-files', '-z', '--', '*Cargo.toml'], indexCheckout(checkout), runner).split('\0').filter(Boolean);
    } catch (error) {
      const valid = runner('git', ['rev-parse', '--is-inside-work-tree'], { cwd: checkout, encoding: 'utf8' });
      if (valid.status === 0) throw error;
      log(`skipped unavailable worktree index: ${checkout}`);
      continue;
    }
    const roots = new Set();
    for (const relative of files.sort((a, b) => a.length - b.length)) {
      const manifest = path.join(checkout, relative);
      if ([...roots].some((root) => key(manifest).startsWith(`${key(root)}${path.sep}`))) continue;
      const metadata = JSON.parse(command('cargo', ['metadata', '--format-version', '1', '--offline', '--no-deps',
        '--manifest-path', manifest], checkout, runner));
      if (typeof metadata.build_directory !== 'string' || !path.isAbsolute(metadata.build_directory)) {
        throw new Error(`Cargo must expose build_directory: ${manifest}`);
      }
      roots.add(metadata.workspace_root);
      mappings.push({ checkout, workspace: metadata.workspace_root, build: path.resolve(metadata.build_directory) });
    }
  }
  return mappings;
}

export function orphanCandidates(root, mappings) {
  const live = new Set(mappings.map((mapping) => key(mapping.build)));
  const candidates = [];
  for (const prefix of fs.readdirSync(root)) {
    if (!/^[a-f0-9]{2}$/.test(prefix)) throw new Error(`unexpected build cache entry: ${prefix}`);
    const bucket = path.join(root, prefix);
    if (fs.lstatSync(bucket).isSymbolicLink() || !fs.statSync(bucket).isDirectory()) throw new Error(`unsafe bucket: ${bucket}`);
    for (const suffix of fs.readdirSync(bucket)) {
      if (!/^[a-f0-9]{14}$/.test(suffix)) throw new Error(`unexpected build cache entry: ${suffix}`);
      const dir = path.join(bucket, suffix);
      if (!live.has(key(dir))) candidates.push(dir);
    }
  }
  return candidates;
}

const HASHED_BUILD = (build) => /^[a-f0-9]{14}$/.test(path.basename(build)) && /^[a-f0-9]{2}$/.test(path.basename(path.dirname(build)));
const SHARED_BUILD = (build) => ['lanes', 'codeterm-lanes'].includes(path.basename(build));

// Superseded variants of each live build directory go under Cargo's own locks; a locked directory is left whole.
export function trimBuildDirectories(mappings, { dryRun = false, runner = spawnSync, plan = retentionPaths, env = process.env } = {}) {
  const python = ['python3', 'python'].find((program) => runner(program, ['--version'], { encoding: 'utf8' }).status === 0);
  if (!python) throw new Error('Python is required for Cargo-compatible build-directory locks');
  const helper = fileURLToPath(new URL('./cargo-build-gc.py', import.meta.url));
  let trimmed = 0;
  const directories = new Map([...mappings.map((mapping) => mapping.build), laneBuildDirectory({ env })]
    .map((build) => [key(build), path.resolve(build)]));
  for (const build of directories.values()) {
    if ((!HASHED_BUILD(build) && !SHARED_BUILD(build)) || !fs.existsSync(build)) continue;
    const paths = [...new Set([...plan(build), ...(SHARED_BUILD(build) ? orphanLanePaths(build) : [])])];
    if (!paths.length) continue;
    const list = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'build-trim-')), 'paths.json');
    try {
      fs.writeFileSync(list, JSON.stringify(paths));
      const root = SHARED_BUILD(build) ? path.dirname(build) : path.dirname(path.dirname(build));
      const result = JSON.parse(command(python, [helper, '--trim', root, build, list, ...(dryRun ? ['--dry-run'] : [])], path.dirname(build), runner));
      log(`${result.status} ${build}: ${(result.bytes / 1024 ** 3).toFixed(3)} GiB${result.reason ? ` (${result.reason})` : ''}`);
      trimmed += result.bytes;
    } finally {
      fs.rmSync(path.dirname(list), { recursive: true, force: true });
    }
  }
  return trimmed;
}

// The X: alias exists only inside a gateway job, so its build directory is recorded whenever it is seen and protected from that record otherwise.
export function resolveStableBuilds(stableMapped, mappings, stable, record) {
  if (stableMapped) {
    const builds = mappings.filter((mapping) => key(mapping.checkout) === key(stable)).map((mapping) => mapping.build);
    if (!builds.length) throw new Error('gateway build-directory mapping is missing');
    if (record) fs.writeFileSync(record, JSON.stringify({ builds }));
    return builds;
  }
  let builds = [];
  try { builds = JSON.parse(fs.readFileSync(record, 'utf8')).builds; } catch { builds = []; }
  if (!Array.isArray(builds) || !builds.length || !builds.every((build) => typeof build === 'string' && path.isAbsolute(build))) {
    throw new Error(`stable gateway path ${stable} is unmapped and no gateway build directory is recorded`);
  }
  log(`stable gateway path unmapped; protecting recorded ${builds.join(', ')}`);
  return builds;
}

export function collectCargoBuilds(checkout, { dryRun = false, runner = spawnSync, record } = {}) {
  const common = command('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], checkout, runner).trim();
  const canonical = path.dirname(common);
  const plugins = process.env.CODETERM_PLUGINS_CHECKOUT || path.join(path.dirname(canonical), 'codeterm-plugins');
  if (!fs.existsSync(plugins)) throw new Error(`plugins checkout missing: ${plugins}; set CODETERM_PLUGINS_CHECKOUT`);
  const stable = process.platform === 'win32' ? `${process.env.CODETERM_STABLE_BUILD_DRIVE || 'X'}:/` : checkout;
  const stableMapped = fs.existsSync(stable);
  const inventory = () => [...worktrees(canonical, runner), ...worktrees(plugins, runner), ...(stableMapped ? [stable] : [])];
  let checkouts = inventory();
  const map = entries => mapCheckouts(entries, { runner, indexCheckout: directory => key(directory) === key(stable) ? checkout : directory });
  let mappings = map(checkouts);
  for (const mapping of mappings) log(`live ${mapping.workspace} -> ${mapping.build}`);
  const stableBuilds = resolveStableBuilds(stableMapped, mappings, stable, record);
  const protectedBuilds = () => [...mappings, ...stableBuilds.map((build) => ({ build }))];
  const roots = new Set(protectedBuilds().map((mapping) => {
    if (!/^[a-f0-9]{14}$/.test(path.basename(mapping.build)) || !/^[a-f0-9]{2}$/.test(path.basename(path.dirname(mapping.build)))) {
      return null;
    }
    return path.dirname(path.dirname(mapping.build));
  }).filter(Boolean));
  let reclaimed = 0;
  const helper = fileURLToPath(new URL('./cargo-build-gc.py', import.meta.url));
  const python = ['python3', 'python'].find((program) => runner(program, ['--version'], { encoding: 'utf8' }).status === 0);
  if (!python) throw new Error('Python is required for Cargo-compatible build-directory locks');
  for (const root of roots) {
    if (!fs.existsSync(root)) continue;
    for (const dir of orphanCandidates(root, protectedBuilds())) {
      const current = inventory();
      if (JSON.stringify(current) !== JSON.stringify(checkouts)) {
        log('worktree inventory changed; remapping before the next removal');
        checkouts = current;
        mappings = map(checkouts);
        if (protectedBuilds().some((mapping) => key(mapping.build) === key(dir))) continue;
      }
      const result = JSON.parse(command(python, [helper, root, dir, ...(dryRun ? ['--dry-run'] : [])], checkout, runner));
      log(`${result.status} ${dir}: ${(result.bytes / 1024 ** 3).toFixed(3)} GiB${result.reason ? ` (${result.reason})` : ''}`);
      reclaimed += result.bytes;
    }
  }
  reclaimed += trimBuildDirectories([...roots].map(root => ({ build: path.join(path.dirname(root), 'lanes') })), { dryRun, runner });
  log(`${dryRun ? 'would free' : 'freed'} ${(reclaimed / 1024 ** 3).toFixed(3)} GiB (${reclaimed} bytes)`);
  return reclaimed;
}
