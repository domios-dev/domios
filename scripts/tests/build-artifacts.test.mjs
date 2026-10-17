import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { copyFileIfChanged, syncBuildDirectory } from '../build-artifacts.cjs';

function fixture(t) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'build-artifacts-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  for (const name of ['source', 'destination']) fs.mkdirSync(path.join(root, name));
  return { root, source: path.join(root, 'source'), destination: path.join(root, 'destination') };
}

test('unchanged build files retain their identity and timestamp', t => {
  const { source, destination } = fixture(t);
  const input = path.join(source, 'binary');
  const output = path.join(destination, 'binary');
  fs.writeFileSync(input, 'same bytes');
  fs.writeFileSync(output, 'same bytes');
  fs.utimesSync(output, 100, 100);
  const before = fs.statSync(output, { bigint: true });
  assert.equal(copyFileIfChanged(input, output), false);
  const after = fs.statSync(output, { bigint: true });
  assert.equal(after.mtimeNs, before.mtimeNs);
  assert.equal(after.ino, before.ino);
});

test('changed build files replace the old bytes and preserve executable permissions', t => {
  const { source, destination } = fixture(t);
  const input = path.join(source, 'binary');
  const output = path.join(destination, 'binary');
  fs.writeFileSync(input, 'new bytes', { mode: 0o755 });
  fs.writeFileSync(output, 'old bytes');
  assert.equal(copyFileIfChanged(input, output), true);
  assert.equal(fs.readFileSync(output, 'utf8'), 'new bytes');
  if (process.platform !== 'win32') assert.equal(fs.statSync(output).mode & 0o777, fs.statSync(input).mode & 0o777);
});

test('frontend sync updates changed assets, retains unchanged files, and removes stale chunks', t => {
  const { source, destination } = fixture(t);
  for (const directory of [source, destination]) fs.mkdirSync(path.join(directory, 'assets'));
  fs.writeFileSync(path.join(source, 'index.html'), 'new index');
  fs.writeFileSync(path.join(source, 'assets/current.js'), 'same chunk');
  fs.writeFileSync(path.join(destination, 'assets/current.js'), 'same chunk');
  fs.utimesSync(path.join(destination, 'assets/current.js'), 100, 100);
  fs.writeFileSync(path.join(destination, 'assets/old.js'), 'obsolete');
  syncBuildDirectory(source, destination);
  assert.equal(fs.readFileSync(path.join(destination, 'index.html'), 'utf8'), 'new index');
  assert.equal(fs.statSync(path.join(destination, 'assets/current.js')).mtimeMs, 100000);
  assert.equal(fs.existsSync(path.join(destination, 'assets/old.js')), false);
});

test('a missing build output cannot delete the last working frontend', t => {
  const { root, destination } = fixture(t);
  fs.writeFileSync(path.join(destination, 'index.html'), 'working frontend');
  assert.throws(() => syncBuildDirectory(path.join(root, 'missing'), destination));
  assert.equal(fs.readFileSync(path.join(destination, 'index.html'), 'utf8'), 'working frontend');
});
