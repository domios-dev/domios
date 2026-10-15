// Caller: scripts/cache-retention.mjs, scripts/cargo-build-gc.test.mjs
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { retentionPaths } from './rust-build-dir-retention.mjs';

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

export function mapCheckouts(checkouts, { runner = spawnSync, manifests } = {}) {
  const mappings = [];
  for (const checkout of [...new Set(checkouts)]) {
    if (!fs.existsSync(checkout)) continue;
    const files = manifests ? manifests(checkout) : command('git', ['ls-files', '-z', '--', '*Cargo.toml'], checkout, runner)
      .split('\0').filter(Boolean);
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
export function trimBuildDirectories(mappings, { dryRun = false, runner = spawnSync, plan = retentionPaths } = {}) {
  const python = ['python3', 'python'].find((program) => runner(program, ['--version'], { encoding: 'utf8' }).status === 0);
  if (!python) throw new Error('Python is required for Cargo-compatible build-directory locks');
  const helper = fileURLToPath(new URL('./cargo-build-gc.py', import.meta.url));
  let trimmed = 0;
  for (const build of [...new Set(mappings.map((mapping) => mapping.build))]) {
    if ((!HASHED_BUILD(build) && !SHARED_BUILD(build)) || !fs.existsSync(build)) continue;
    const paths = plan(build);
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

export function collectCargoBuilds(checkout, { dryRun = false, runner = spawnSync } = {}) {
  const common = command('git', ['rev-parse', '--path-format=absolute', '--git-common-dir'], checkout, runner).trim();
  const canonical = path.dirname(common);
  const plugins = process.env.CODETERM_PLUGINS_CHECKOUT || path.join(path.dirname(canonical), 'codeterm-plugins');
  if (!fs.existsSync(plugins)) throw new Error(`plugins checkout missing: ${plugins}; set CODETERM_PLUGINS_CHECKOUT`);
  const stable = process.platform === 'win32' ? `${process.env.CODETERM_STABLE_BUILD_DRIVE || 'X'}:/` : checkout;
  if (!fs.existsSync(stable)) throw new Error(`stable gateway path unavailable: ${stable}`);
  const checkouts = [...worktrees(canonical, runner), ...worktrees(plugins, runner), stable];
  const checkoutSnapshot = JSON.stringify(checkouts);
  const mappings = mapCheckouts(checkouts, { runner });
  for (const mapping of mappings) log(`live ${mapping.workspace} -> ${mapping.build}`);
  const stableMappings = mappings.filter((mapping) => key(mapping.checkout) === key(stable));
  if (!stableMappings.length) throw new Error('gateway build-directory mapping is missing');
  const roots = new Set(mappings.map((mapping) => {
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
    for (const dir of orphanCandidates(root, mappings)) {
      if (JSON.stringify([...worktrees(canonical, runner), ...worktrees(plugins, runner), stable]) !== checkoutSnapshot) {
        throw new Error('worktree inventory changed during GC; preview the new inventory before retrying');
      }
      const result = JSON.parse(command(python, [helper, root, dir, ...(dryRun ? ['--dry-run'] : [])], checkout, runner));
      log(`${result.status} ${dir}: ${(result.bytes / 1024 ** 3).toFixed(3)} GiB${result.reason ? ` (${result.reason})` : ''}`);
      reclaimed += result.bytes;
    }
  }
  reclaimed += trimBuildDirectories(mappings, { dryRun, runner });
  log(`${dryRun ? 'would free' : 'freed'} ${(reclaimed / 1024 ** 3).toFixed(3)} GiB (${reclaimed} bytes)`);
  return reclaimed;
}
