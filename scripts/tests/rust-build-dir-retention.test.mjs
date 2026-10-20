import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { planRetention, scanBuildDir, unitIdentity, unitPaths } from '../rust-build-dir-retention.mjs';

const keep = { units: 2, incremental: 2 };
const unit = (identity, mtime, name = 'codeterm-util', profile = 'debug') => ({ profile, name, identity, mtime, hash: mtime.toString(16).padStart(16, '0') });

test('identity reads the unit file names and identity fields, ignoring dependency hashes', () => {
  const json = { 'lib-codeterm_util.json': { rustc: 1, features: '[]', profile: 7, path: 9, deps: [[1, 'a', false, 5]] } };
  const other = { 'lib-codeterm_util.json': { ...json['lib-codeterm_util.json'], deps: [[1, 'a', false, 6]] } };
  const files = ['lib-codeterm_util', 'lib-codeterm_util.json', 'invoked.timestamp'];
  assert.equal(unitIdentity(files, (n) => json[n]), unitIdentity(files, (n) => other[n]));
  const featured = { 'lib-codeterm_util.json': { ...json['lib-codeterm_util.json'], features: '["x"]' } };
  assert.notEqual(unitIdentity(files, (n) => json[n]), unitIdentity(files, (n) => featured[n]));
  assert.equal(unitIdentity(['invoked.timestamp'], () => null), null);
});

test('only superseded builds of one identity go; a busy test kind never evicts the dev kind', () => {
  const units = [...[1, 2, 3, 4, 5].map((t) => unit('test', t)), unit('dev', 1), unit('dev', 1, 'codeterm-util', 'release'), unit(null, 0)];
  const removed = planRetention({ units, sessions: [] }, keep).units;
  assert.deepEqual(removed.map((u) => [u.identity, u.mtime]), [['test', 3], ['test', 2], ['test', 1]]);
});

test('incremental sessions age per crate on their own count', () => {
  const sessions = [5, 1, 3].map((mtime) => ({ profile: 'debug', name: 'codeterm_types', mtime }));
  assert.deepEqual(planRetention({ units: [], sessions }, keep).sessions.map((e) => e.mtime), [1]);
});

test('a removed unit takes its fingerprint, deps files and build-script dir of that hash only', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'build-dir-'));
  try {
    for (const h of ['aaaaaaaaaaaaaaaa', 'bbbbbbbbbbbbbbbb']) {
      const fp = path.join(root, 'debug', '.fingerprint', `codeterm-util-${h}`);
      fs.mkdirSync(fp, { recursive: true });
      fs.writeFileSync(path.join(fp, 'lib-codeterm_util.json'), JSON.stringify({ rustc: 1, features: '[]' }));
      fs.mkdirSync(path.join(root, 'debug', 'deps'), { recursive: true });
      fs.writeFileSync(path.join(root, 'debug', 'deps', `libcodeterm_util-${h}.rlib`), '');
      fs.writeFileSync(path.join(root, 'debug', 'deps', `codeterm_util-${h}.d`), '');
      fs.mkdirSync(path.join(root, 'debug', 'build', `codeterm-util-${h}`, 'out'), { recursive: true });
    }
    fs.mkdirSync(path.join(root, 'debug', 'build', 'libsqlite3-sys-cccccccccccccccc', 'out'), { recursive: true });
    fs.mkdirSync(path.join(root, 'debug', 'incremental', 'codeterm_util-1x2y3z'), { recursive: true });
    const scanned = scanBuildDir(root);
    assert.deepEqual([scanned.units.length, scanned.sessions.length], [2, 1]);
    assert.ok(scanned.units.every((u) => u.identity));
    const target = scanned.units.find((u) => u.hash === 'aaaaaaaaaaaaaaaa');
    const paths = unitPaths(root, target).map((p) => path.relative(root, p).replaceAll('\\', '/')).sort();
    assert.deepEqual(paths, [
      'debug/.fingerprint/codeterm-util-aaaaaaaaaaaaaaaa',
      'debug/build/codeterm-util-aaaaaaaaaaaaaaaa',
      'debug/deps/codeterm_util-aaaaaaaaaaaaaaaa.d',
      'debug/deps/libcodeterm_util-aaaaaaaaaaaaaaaa.rlib',
    ]);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
