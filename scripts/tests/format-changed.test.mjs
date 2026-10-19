import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { afterEach, test } from 'node:test';
import { collectChangedFiles, formatChangedFiles } from '../format-changed.mjs';

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

  assert.deepEqual(collectChangedFiles(root), [
    'committed.ts', 'staged.ts', 'unstaged.ts', 'untracked.ts',
  ]);
});

test('formats a changed Rust file with its package edition', () => {
  const root = mkdtempSync(path.join(os.tmpdir(), 'codeterm-fmt-rust-'));
  temporaryDirectories.push(root);
  git(root, 'init', '-b', 'develop');
  git(root, 'config', 'user.name', 'Fixture');
  git(root, 'config', 'user.email', 'fixture@example.invalid');
  write(root, 'src-tauri/Cargo.toml', '[package]\nname = "fmt-fixture"\nversion = "0.1.0"\nedition = "2018"\n');
  write(root, 'src-tauri/src/lib.rs', 'pub fn answer() -> u32 { 0 }\n');
  git(root, 'add', '.');
  git(root, 'commit', '-m', 'base');
  git(root, 'branch', 'origin/develop');
  git(root, 'switch', '-c', 'work');
  write(root, 'src-tauri/src/lib.rs', 'pub fn answer()->u32{42}\n');
  git(root, 'add', '.');
  git(root, 'commit', '-m', 'changed Rust source');

  const result = formatChangedFiles(root);
  assert.deepEqual(result.rustFiles, ['src-tauri/src/lib.rs']);
  assert.deepEqual(result.biomeFiles, []);
  assert.match(readFileSync(path.join(root, 'src-tauri/src/lib.rs'), 'utf8'), /pub fn answer\(\) -> u32 \{\n    42\n\}/);
});
