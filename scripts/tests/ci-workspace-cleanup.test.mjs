import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { removeJobOutputs } from '../ci-workspace-cleanup.mjs';

function workspace() {
  const root = mkdtempSync(join(tmpdir(), 'domios-ci-cleanup-'));
  const ws = join(root, 'ws');
  for (const dir of ['.git', 'src-tauri/target/debug', 'cli/target', 'ci-rust-receipts', 'src']) mkdirSync(join(ws, dir), { recursive: true });
  writeFileSync(join(ws, 'ci-rust-receipts/rust-tests-full-Linux.json'), '{}');
  writeFileSync(join(ws, 'src/main.rs'), 'fn main() {}');
  return { root, ws };
}

test('removes job targets and archives and keeps the checkout', () => {
  const { root, ws } = workspace();
  try {
    const removed = removeJobOutputs(ws);
    assert.deepEqual(removed, ['src-tauri/target', 'cli/target', 'ci-rust-receipts']);
    for (const gone of ['src-tauri/target', 'cli/target', 'ci-rust-receipts']) assert.equal(existsSync(join(ws, gone)), false, gone);
    for (const kept of ['.git', 'src/main.rs']) assert.ok(existsSync(join(ws, kept)), kept);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('refuses paths that leave the workspace or touch .git', () => {
  const { root, ws } = workspace();
  try {
    for (const entry of ['../outside', '/etc', '.git', 'src/../../outside']) assert.throws(() => removeJobOutputs(ws, [entry]), /refusing/, entry);
    assert.ok(existsSync(join(ws, '.git')));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('a symlinked target is unlinked without deleting what it points to', (t) => {
  const { root, ws } = workspace();
  try {
    const shared = join(root, 'shared-target');
    mkdirSync(join(shared, 'debug'), { recursive: true });
    try { symlinkSync(shared, join(ws, 'target'), 'junction'); } catch { t.skip('symlinks unavailable'); return; }
    assert.ok(removeJobOutputs(ws, ['target']).includes('target'));
    assert.equal(existsSync(join(ws, 'target')), false);
    assert.ok(existsSync(join(shared, 'debug')));
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('a symlinked parent that escapes the workspace is refused', (t) => {
  const { root, ws } = workspace();
  try {
    const outside = join(root, 'outside');
    mkdirSync(join(outside, 'target'), { recursive: true });
    try { symlinkSync(outside, join(ws, 'linked'), 'junction'); } catch { t.skip('symlinks unavailable'); return; }
    assert.throws(() => removeJobOutputs(ws, ['linked/target']), /outside/);
    assert.ok(existsSync(join(outside, 'target')));
  } finally { rmSync(root, { recursive: true, force: true }); }
});
