// Caller: scripts/cache-retention.mjs, scripts/cargo-build-gc.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

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
  log(`${dryRun ? 'would free' : 'freed'} ${(reclaimed / 1024 ** 3).toFixed(3)} GiB (${reclaimed} bytes)`);
  return reclaimed;
}
