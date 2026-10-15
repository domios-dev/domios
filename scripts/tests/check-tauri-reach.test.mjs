import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { baselinePath, checkReach, countReach, scope, validateBaseline } from '../check-tauri-reach.mjs';

const file = `${scope}service/example.rs`;
const document = files => ({ version: 1, files });

test('counts tauri reach while allowing the core runtime replacement', () => {
  assert.equal(countReach('tauri::async_runtime::spawn(); AppHandle app_handle'), 3);
  assert.equal(countReach('crate::async_runtime::spawn(); tokio::runtime::Handle'), 0);
});

test('requires exact positive per-file budgets and rejects growth or stale entries', () => {
  assert.deepEqual(checkReach({ [file]: 2 }, { [file]: 2 }), []);
  assert.match(checkReach({ [file]: 3 }, { [file]: 2 }).join(), /grew/);
  assert.match(checkReach({}, { [file]: 2 }).join(), /drop entry/);
  assert.deepEqual(checkReach({}, {}, { [file]: 2 }), []);
  assert.match(checkReach({ [file]: 3 }, { [file]: 3 }, { [file]: 2 }).join(), /increased baseline/);
  for (const [path, count] of [[file, 0], [file, -1], [file, 1.5], [`${scope}../escape.rs`, 1], [`${scope}*.rs`, 1], ['src-tauri/src/main.rs', 1]]) {
    assert.throws(() => validateBaseline(document({ [path]: count })));
  }
});

test('CLI fixture ratchets committed budgets and rejects restoring removed reach', () => {
  const root = mkdtempSync(join(tmpdir(), 'tauri-reach-'));
  const checker = fileURLToPath(new URL('../check-tauri-reach.mjs', import.meta.url));
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  const source = (path, text) => {
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  };
  const baseline = files => source(baselinePath, JSON.stringify(document(files)));
  const commit = () => {
    git('add', '-A');
    git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'fixture');
  };
  const run = (base = 'HEAD') => spawnSync(process.execPath, [checker, '--base', base], { cwd: root, encoding: 'utf8' });
  try {
    git('init', '--quiet');
    source(file, 'tauri::async_runtime::spawn(task);');
    baseline({ [file]: 1 });
    commit();
    assert.equal(run().status, 0);
    const crash = 'src-tauri/crates/codeterm-crash/src/nested/leak.rs';
    source(crash, 'use tauri::Runtime;');
    assert.match(run().stderr, /crash.*reach grew/);
    source(crash, 'pub fn clean() {}');
    assert.equal(run().status, 0);
    source(file, 'tauri::async_runtime::spawn(task); AppHandle');
    assert.match(run().stderr, /grew/);
    baseline({ [file]: 2 });
    assert.match(run().stderr, /increased baseline/);
    source(file, 'crate::async_runtime::spawn(task);');
    baseline({ [file]: 1 });
    assert.match(run().stderr, /drop entry/);
    baseline({});
    assert.equal(run().status, 0);
    commit();
    source(file, 'tauri::async_runtime::spawn(task);');
    baseline({ [file]: 1 });
    assert.match(run().stderr, /increased baseline/);
    assert.equal(run('missing-ref').status, 1);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
