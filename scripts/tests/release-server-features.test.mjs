import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { assertReleaseServerFeatures, checkReleaseServerFeatures } from '../release-stage.mjs';

const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');

test('release build and dependency tree match the staged server feature contract', () => {
  checkReleaseServerFeatures(source);
  assertReleaseServerFeatures(['mem-postgres', 'embeddings']);
});

test('staging rejects missing, extra and duplicate server features', () => {
  for (const features of [[], ['embeddings'], ['mem-postgres'],
    ['embeddings', 'mem-postgres', 'default'], ['embeddings', 'mem-postgres', 'mem-postgres']]) {
    assert.throws(() => assertReleaseServerFeatures(features), /feature selection/);
  }
});

test('preflight rejects feature drift in either Cargo command', () => {
  for (const operation of ['build', 'tree']) {
    const drift = source.replace(new RegExp(`(cargo ${operation} --[^]*?-p domios-server[^]*?--features )domios-server/embeddings,domios-server/mem-postgres`, 'g'), '$1domios-server/embeddings');
    assert.notEqual(drift, source);
    assert.throws(() => checkReleaseServerFeatures(drift), /feature selection/);
  }
});

test('preflight refuses an unrecognised or non-isolated release build', () => {
  assert.throws(() => checkReleaseServerFeatures(''), /function is missing/);
  const inputs = source.replaceAll('\r\n', '\n')
    .match(/^prepare_release_bundle_inputs\(\) \{\n[\s\S]*?^\}/m)[0];
  assert.throws(() => checkReleaseServerFeatures(inputs.replace(/cargo tree/g, 'cargo metadata')), /cargo tree command/);
  assert.throws(() => checkReleaseServerFeatures(inputs.replaceAll('--no-default-features', '')), /isolated features/);
});
