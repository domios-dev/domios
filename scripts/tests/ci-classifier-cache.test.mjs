import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { classificationKey, classifyTarget } from '../ci-classifier-cache.mjs';

function fixture() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'classifier-cache-'));
  const src = path.join(root, 'crate', 'src');
  fs.mkdirSync(path.join(src, 'nested'), { recursive: true });
  fs.writeFileSync(path.join(src, 'lib.rs'), 'mod nested;');
  fs.writeFileSync(path.join(src, 'nested', 'mod.rs'), '#[test] fn opaque() {}');
  return { root, source: path.join(src, 'lib.rs'), nested: path.join(src, 'nested', 'mod.rs') };
}

test('the key follows every file the classifier could read, and only those', () => {
  const { root, source, nested } = fixture();
  try {
    const first = classificationKey(source);
    assert.equal(classificationKey(source), first);
    fs.writeFileSync(path.join(root, 'crate', 'Cargo.toml'), '[package]');
    assert.equal(classificationKey(source), first, 'files outside the source directory do not matter');
    fs.writeFileSync(nested, '#[cfg(windows)] #[test] fn opaque() {}');
    assert.notEqual(classificationKey(source), first, 'a module file is part of the key');
    const generated = path.join(root, 'out');
    fs.mkdirSync(generated);
    fs.writeFileSync(path.join(generated, 'tests.rs'), 'opaque');
    assert.notEqual(classificationKey(source, generated), classificationKey(source));
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('a target is classified once: then from this runner, and on a fresh runner from the store', () => {
  const { root, source } = fixture();
  const store = new Map();
  const load = (key, destination) => store.has(key) && (fs.writeFileSync(destination, store.get(key)), true);
  const save = (key, file) => store.set(key, fs.readFileSync(file, 'utf8'));
  let runs = 0;
  const run = () => { runs++; return JSON.stringify([{ name: 'opaque', platform: true }]); };
  try {
    const stats = { local: 0, store: 0, classified: 0 };
    const env = { CI_CLASSIFIER_CACHE: path.join(root, 'runner-a') };
    assert.deepEqual(classifyTarget(source, undefined, { env, run, load, save, stats }), [{ name: 'opaque', platform: true }]);
    classifyTarget(source, undefined, { env, run, load, save, stats });
    classifyTarget(source, undefined, { env: { CI_CLASSIFIER_CACHE: path.join(root, 'runner-b') }, run, load, save, stats });
    assert.equal(runs, 1);
    assert.deepEqual(stats, { local: 1, store: 1, classified: 1 });
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('a classifier failure is never cached', () => {
  const { root, source } = fixture();
  const env = { CI_CLASSIFIER_CACHE: path.join(root, 'cache') };
  let runs = 0;
  const run = () => { runs++; throw new Error('include requires Cargo build-script OUT_DIR'); };
  try {
    for (let i = 0; i < 2; i++) assert.throws(() => classifyTarget(source, undefined, { env, run, load: () => false, save: () => {} }), /OUT_DIR/);
    assert.equal(runs, 2);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
