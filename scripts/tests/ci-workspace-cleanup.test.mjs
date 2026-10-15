import assert from 'node:assert/strict';
import { test } from 'node:test';
import { mkdtempSync, mkdirSync, writeFileSync, existsSync, symlinkSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { removeJobOutputs, sweepRustTargets } from '../ci-workspace-cleanup.mjs';

function workspace() {
  const root = mkdtempSync(join(tmpdir(), 'domios-ci-cleanup-'));
  const ws = join(root, 'ws');
  for (const dir of ['.git', 'src-tauri/target/debug', 'cli/target', 'ci-rust-receipts', 'src']) mkdirSync(join(ws, dir), { recursive: true });
  writeFileSync(join(ws, 'ci-rust-receipts/rust-tests-full-Linux.json'), '{}');
  writeFileSync(join(ws, 'src/main.rs'), 'fn main() {}');
  return { root, ws };
}

test('removes job archives and keeps the checkout and the warm Rust targets', () => {
  const { root, ws } = workspace();
  try {
    assert.deepEqual(removeJobOutputs(ws), ['ci-rust-receipts']);
    assert.equal(existsSync(join(ws, 'ci-rust-receipts')), false);
    for (const kept of ['.git', 'src/main.rs', 'src-tauri/target/debug', 'cli/target']) assert.ok(existsSync(join(ws, kept)), kept);
  } finally { rmSync(root, { recursive: true, force: true }); }
});

test('sweeps each existing Rust target by age and keeps a target when cargo-sweep is missing', () => {
  const { root, ws } = workspace();
  try {
    const calls = [];
    const swept = sweepRustTargets(ws, (command, args) => { calls.push([command, ...args]); return { status: 0 }; }, undefined);
    assert.deepEqual(swept, ['src-tauri', 'cli']);
    assert.deepEqual(calls.map(call => call.slice(0, 4)), [['cargo', 'sweep', '--time', '7'], ['cargo', 'sweep', '--time', '7']]);
    assert.deepEqual(sweepRustTargets(ws, () => ({ error: new Error('missing') }), undefined), []);
    assert.ok(existsSync(join(ws, 'src-tauri/target/debug')));
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

test('sweeps warm targets kept outside the checkout with their own CARGO_TARGET_DIR', () => {
  const { root, ws } = workspace();
  try {
    for (const dir of ['src-tauri', 'cli']) writeFileSync(join(ws, dir, 'Cargo.toml'), '[package]');
    const external = join(root, 'ci-targets', 'tests');
    for (const dir of ['src-tauri', 'cli', 'orphan']) mkdirSync(join(external, dir), { recursive: true });
    rmSync(join(ws, 'src-tauri/target'), { recursive: true }); rmSync(join(ws, 'cli/target'), { recursive: true });
    const seen = [];
    sweepRustTargets(ws, (command, args, options) => { seen.push([args.at(-1), options.env.CARGO_TARGET_DIR]); return { status: 0 }; }, external);
    assert.deepEqual(seen.sort(), [[join(ws, 'cli'), join(external, 'cli')], [join(ws, 'src-tauri'), join(external, 'src-tauri')]]);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
