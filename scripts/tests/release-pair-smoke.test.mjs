import { strict as assert } from 'node:assert';
import test from 'node:test';
import { parseApiToken, validateReady } from '../release-pair-smoke.mjs';

test('isolated smoke accepts only exact server READY identity', () => {
  assert.deepEqual(validateReady('{"type":"READY","build":"abc","port":49152}', 'abc', 49152),
    { type: 'READY', build: 'abc', port: 49152 });
  assert.throws(() => validateReady('{"type":"READY","build":"other","port":49152}', 'abc', 49152));
  assert.throws(() => validateReady('{"type":"READY","build":"abc","port":49152,"token":"secret"}', 'abc', 49152));
});

test('isolated smoke reads the API token only from the api TOML section', () => {
  assert.equal(parseApiToken('[other]\ntoken = "wrong"\n[api]\ntoken = "secret-token"\n'), 'secret-token');
  assert.throws(() => parseApiToken('[other]\ntoken = "wrong"\n'));
  assert.throws(() => parseApiToken('[api]\ntoken = "one"\ntoken = "two"\n'));
});
