import assert from 'node:assert/strict';
import test from 'node:test';
import { promotionPlan, publishConcurrently, verifyAssetDigests } from './release-promote.mjs';

const files = [
  { name: 'Domios.AppImage', sha256: 'a'.repeat(64), size: 42 },
  { name: 'updater-linux-x86_64.json', sha256: 'b'.repeat(64), size: 12 },
  { name: 'domios-headless.tar.gz', sha256: 'c'.repeat(64), size: 84 },
  { name: 'domios-headless.tar.gz.sha256', sha256: 'd'.repeat(64), size: 90 },
  { name: 'domios-headless.json', sha256: 'e'.repeat(64), size: 20 },
];
const asset = file => ({ name: file.name, digest: `sha256:${file.sha256}`, size: file.size, state: 'uploaded' });

test('promotion includes every retained asset and headless metadata; only source gets fragments', () => {
  const plan = promotionPlan({ files });
  assert.deepEqual(plan.source, files.map(file => file.name));
  assert.deepEqual(plan.mirror, files.filter(file => !file.name.startsWith('updater-')).map(file => file.name));
});

test('API digests match all receipt files regardless of API order or unrelated assets', () => {
  assert.doesNotThrow(() => verifyAssetDigests(files, [...files.map(asset).reverse(), { name: 'other' }], 'owner/repo'));
});

test('missing, malformed, or non-sha256 digests refuse publication', () => {
  for (const digest of [undefined, null, '', 'sha256:', 'md5:' + 'a'.repeat(64), 'sha256:' + 'z'.repeat(64)]) {
    assert.throws(() => verifyAssetDigests([files[0]], [{ ...asset(files[0]), digest }], 'owner/repo'), /no valid sha256 API digest/);
  }
});

test('missing and duplicate assets refuse publication', () => {
  assert.throws(() => verifyAssetDigests(files, files.slice(1).map(asset), 'owner/repo'), /expected one uploaded asset/);
  assert.throws(() => verifyAssetDigests([files[0]], [asset(files[0]), asset(files[0])], 'owner/repo'), /expected one uploaded asset/);
});

test('changed digests, sizes, states, or invalid receipt digests refuse publication', () => {
  for (const changed of [
    { digest: 'sha256:' + 'f'.repeat(64) }, { size: 43 }, { state: 'starter' },
  ]) {
    assert.throws(() => verifyAssetDigests([files[0]], [{ ...asset(files[0]), ...changed }], 'owner/repo'), /differs from its retained receipt/);
  }
  assert.throws(() => verifyAssetDigests([{ ...files[0], sha256: null }], [asset(files[0])], 'owner/repo'), /differs from its retained receipt/);
});

test('publication starts three concurrent tasks and starts another only when a slot is free', async () => {
  const started = [];
  const releases = [];
  const fourthStarted = Promise.withResolvers();
  const work = publishConcurrently([0, 1, 2, 3], async task => {
    started.push(task);
    const gate = Promise.withResolvers();
    releases[task] = gate.resolve;
    if (task === 3) fourthStarted.resolve();
    await gate.promise;
  });
  assert.deepEqual(started, [0, 1, 2]);
  releases[1]();
  await fourthStarted.promise;
  assert.deepEqual(started, [0, 1, 2, 3]);
  for (const release of releases) release();
  await work;
});

test('failure stops new tasks and drains active publications before refusing', async () => {
  const gate = Promise.withResolvers();
  const started = [];
  let drained = false;
  const failure = new Error('upload refused');
  const work = publishConcurrently([0, 1, 2], async task => {
    started.push(task);
    if (task === 0) throw failure;
    await gate.promise;
    drained = true;
  }, 2);
  assert.deepEqual(started, [0, 1]);
  gate.resolve();
  await assert.rejects(work, error => error === failure);
  assert.equal(drained, true);
  assert.deepEqual(started, [0, 1]);
});

test('invalid concurrency refuses without starting work', async () => {
  for (const limit of [0, -1, 1.5, Infinity]) {
    await assert.rejects(publishConcurrently([1], () => assert.fail('unexpected publication'), limit), /invalid publication concurrency/);
  }
});
