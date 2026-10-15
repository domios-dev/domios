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
export const SWEEP_DAYS = process.env.CI_TARGET_SWEEP_DAYS || '2';

export function sweepRustTargets(workspace, run = spawnSync) {
  const swept = [];
  for (const project of RUST_PROJECTS) {
    const dir = path.resolve(workspace, project);
    if (!fs.existsSync(path.join(dir, 'target'))) continue;
    const result = run('cargo', ['sweep', '--time', SWEEP_DAYS, dir], { stdio: 'inherit' });
    if (result.error || result.status !== 0) {
      console.log(`cargo-sweep unavailable or failed for ${project}; target kept`);
      continue;
    }
    swept.push(project);
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
