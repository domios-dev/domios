#!/usr/bin/env node
// Caller: CI tests.yml, release-macos-build.yml, release-linux.yml, release-linux-arm64.yml, release-windows.yml; scripts tests/ci-workspace-cleanup.test.mjs; skill github-release
// Removes a finished job's archives and sweeps its Rust targets by age, so the next job starts warm
// while unused artifacts never accumulate; the git checkout stays.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const JOB_OUTPUTS = ['ci-rust-receipts', 'release-artifacts'];
export const RUST_PROJECTS = ['.', 'src-tauri', 'cli', 'scripts/ci-test-classifier'];
export const SWEEP_DAYS = process.env.CI_TARGET_SWEEP_DAYS || '7';

// Targets kept outside the checkout are named after their workspace directory (src-tauri, cli).
function externalTargets(workspace, root) {
  if (!root || !fs.existsSync(root)) return [];
  return fs.readdirSync(root, { withFileTypes: true }).filter(entry => entry.isDirectory())
    .map(entry => ({
      project: [entry.name, entry.name.replace(/-/g, '/')].find(name => fs.existsSync(path.join(workspace, name, 'Cargo.toml'))),
      target: path.join(root, entry.name),
    }))
    .filter(({ project }) => project);
}

export function sweepRustTargets(workspace, run = spawnSync, root = process.env.CI_TARGET_ROOT) {
  const swept = [];
  const candidates = [
    ...RUST_PROJECTS.filter(project => fs.existsSync(path.join(workspace, project, 'target'))).map(project => ({ project })),
    ...externalTargets(workspace, root),
  ];
  for (const { project, target } of candidates) {
    const dir = path.resolve(workspace, project);
    const env = target ? { ...process.env, CARGO_TARGET_DIR: target } : process.env;
    const result = run('cargo', ['sweep', '--time', SWEEP_DAYS, dir], { stdio: 'inherit', env });
    if (result.error || result.status !== 0) {
      console.log(`cargo-sweep unavailable or failed for ${project}; target kept`);
      continue;
    }
    swept.push(target ? `${project} (${target})` : project);
  }
  return swept;
}

const inside = (root, candidate) => {
  const relative = path.relative(root, candidate);
  return relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative);
};

export function removeJobOutputs(workspace, entries = JOB_OUTPUTS) {
  const root = fs.realpathSync(workspace);
  const removed = [];
  for (const entry of entries) {
    if (path.isAbsolute(entry) || entry.split(/[\\/]/).some((part) => part === '..' || part === '.git')) {
      throw new Error(`refusing ${entry}: only workspace-relative paths outside .git`);
    }
    const target = path.resolve(root, entry);
    let stat;
    try { stat = fs.lstatSync(target); } catch { continue; }
    // A symlinked parent could point outside the workspace; resolve it before deleting.
    const resolvedParent = fs.realpathSync(path.dirname(target));
    const resolved = path.join(resolvedParent, path.basename(target));
    if (!inside(root, resolved)) throw new Error(`refusing ${entry}: resolves outside ${root}`);
    fs.rmSync(resolved, { recursive: !stat.isSymbolicLink(), force: true, maxRetries: 3 });
    removed.push(entry);
  }
  return removed;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const workspace = process.env.GITHUB_WORKSPACE;
  if (!workspace) throw new Error('GITHUB_WORKSPACE is required');
  const extra = process.argv.slice(2);
  const removed = removeJobOutputs(workspace, extra.length ? [...JOB_OUTPUTS, ...extra] : JOB_OUTPUTS);
  console.log(removed.length ? `Removed ${removed.join(', ')}` : 'No job outputs to remove');
  const swept = sweepRustTargets(workspace);
  console.log(swept.length ? `Swept Rust targets older than ${SWEEP_DAYS} days in ${swept.join(', ')}` : 'No Rust targets swept');
}
