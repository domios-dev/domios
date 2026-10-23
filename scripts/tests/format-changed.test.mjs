import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, test } from 'node:test';
import { argumentBatches, biomeCommand, defaultBase, collectChangedFiles, formatChangedFiles } from '../format-changed.mjs';

const temporaryDirectories = [];

function git(root, ...args) {
  execFileSync('git', ['-C', root, ...args], { stdio: 'ignore' });
}

function write(root, relativePath, contents = 'fixture\n') {
  const absolutePath = path.join(root, relativePath);
  mkdirSync(path.dirname(absolutePath), { recursive: true });
  writeFileSync(absolutePath, contents);
}

afterEach(() => {
  for (const directory of temporaryDirectories.splice(0)) {
    rmSync(directory, { recursive: true, force: true });
  }
});

test('selects committed, staged, unstaged and untracked files while skipping deletions and excluded paths', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'codeterm-fmt-selection-'));
  temporaryDirectories.push(root);
  git(root, 'init', '-b', 'develop');
  git(root, 'config', 'user.name', 'Fixture');
  git(root, 'config', 'user.email', 'fixture@example.invalid');
  write(root, 'base.txt');
  write(root, 'deleted.txt');
  write(root, 'staged.ts');
  write(root, 'unstaged.ts');
  write(root, 'ignored-by-size-check/vendor/generated.rs');
  git(root, 'add', '.');
  git(root, 'commit', '-m', 'base');
  git(root, 'branch', 'origin/develop');
  git(root, 'switch', '-c', 'work');

  write(root, 'committed.ts', 'export const committed = 1;\n');
  git(root, 'add', 'committed.ts');
  git(root, 'commit', '-m', 'committed change');
  write(root, 'staged.ts', 'export const staged = 2;\n');
  git(root, 'add', 'staged.ts');
  write(root, 'unstaged.ts', 'export const unstaged = 3;\n');
  git(root, 'rm', 'deleted.txt');
  write(root, 'untracked.ts');
  write(root, 'vendor/new.ts');
  write(root, 'target/new.ts');

  assert.deepEqual(collectChangedFiles(root, 'develop'), [
    'committed.ts', 'staged.ts', 'unstaged.ts', 'untracked.ts',
  ]);
});

test('formats package sources and external example modules with their package edition', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'codeterm-fmt-rust-'));
  temporaryDirectories.push(root);
  write(root, 'rust-toolchain.toml', readFileSync(new URL('../../rust-toolchain.toml', import.meta.url), 'utf8'));
  git(root, 'init', '-b', 'develop');
  git(root, 'config', 'user.name', 'Fixture');
  git(root, 'config', 'user.email', 'fixture@example.invalid');
  write(root, 'src-tauri/Cargo.toml', '[package]\nname = "fmt-fixture"\nversion = "0.1.0"\nedition = "2018"\n\n[[example]]\nname = "external"\npath = "../evals/runner/main.rs"\n');
  write(root, 'src-tauri/src/lib.rs', 'pub fn answer() -> u32 { 0 }\n');
  write(root, 'evals/runner/main.rs', 'mod helper;\nfn main() {}\n');
  write(root, 'evals/runner/helper.rs', 'pub async fn answer() -> u32 { 0 }\n');
  git(root, 'add', '.');
  git(root, 'commit', '-m', 'base');
  git(root, 'branch', 'origin/develop');
  git(root, 'switch', '-c', 'work');
  write(root, 'src-tauri/src/lib.rs', 'pub fn answer()->u32{42}\n');
  write(root, 'evals/runner/helper.rs', 'pub async fn answer()->u32{42}\n');
  git(root, 'add', '.');
  git(root, 'commit', '-m', 'changed Rust source');

  const result = formatChangedFiles(root, 'develop');
  assert.deepEqual(result.rustFiles, ['evals/runner/helper.rs', 'src-tauri/src/lib.rs']);
  assert.deepEqual(result.biomeFiles, []);
  assert.match(readFileSync(path.join(root, 'src-tauri/src/lib.rs'), 'utf8'), /pub fn answer\(\) -> u32 \{\n    42\n\}/);
  assert.match(readFileSync(path.join(root, 'evals/runner/helper.rs'), 'utf8'), /pub async fn answer\(\) -> u32 \{\n    42\n\}/);
});

test('batches quoted Windows paths within the budget without losing order', () => {
  const files = Array.from({ length: 700 }, (_, i) => `src/path with spaces/quoted-"${i}.tsx`);
  const prefix = ['node.exe', 'C:/long path/biome', 'check'];
  const batches = argumentBatches(files, prefix, 1000);
  assert.ok(batches.length > 20);
  assert.deepEqual(batches.flat(), files);
  for (const batch of batches) assert.ok([...prefix, ...batch].reduce((n, s) => n + s.length * 2 + 3, 1) <= 1000);
  assert.throws(() => argumentBatches(['x'.repeat(1000)], prefix, 1000), /exceeds command-line budget/);
});

test('missing Biome names the lane and gives an installation hint', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'fmt-no-biome-'));
  temporaryDirectories.push(root);
  assert.throws(() => biomeCommand(root), error => error.message.includes(root) && error.message.includes('pnpm install --frozen-lockfile'));
});

test('default base follows canonical HEAD instead of a stale remote', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'fmt-canonical-'));
  temporaryDirectories.push(root);
  git(root, 'init', '-b', 'develop');
  git(root, 'config', 'user.name', 'Fixture');
  git(root, 'config', 'user.email', 'fixture@example.invalid');
  write(root, 'base.txt');
  git(root, 'add', '.');
  git(root, 'commit', '-m', 'base');
  git(root, 'branch', 'origin/develop');
  write(root, 'canonical.ts');
  git(root, 'add', '.');
  git(root, 'commit', '-m', 'canonical advance');
  const lane = path.join(root, 'lane');
  git(root, 'worktree', 'add', '-b', 'lane-work', lane);
  assert.deepEqual(collectChangedFiles(lane, defaultBase(lane, {})), []);
  assert.equal(defaultBase(lane, { CODETERM_LANE_BASE: 'explicit-fixture' }), 'explicit-fixture');
  write(lane, 'own.ts');
  assert.deepEqual(collectChangedFiles(lane, defaultBase(lane, {})), ['own.ts']);
});
