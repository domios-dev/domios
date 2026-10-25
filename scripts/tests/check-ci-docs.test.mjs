import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import test from 'node:test';
import { documentReferenceErrors, staleMapSources } from '../check-ci-docs.mjs';

const quote = String.fromCharCode(96);
test('document reference guard rejects duplicate, unknown and self targets without compiling', () => {
  const link = name => quote + 'domios docs ' + name + quote;
  assert.deepEqual(documentReferenceErrors({ OPAQUE: link('OTHER'), OTHER: 'body' }), []);
  for (const body of [link('OTHER') + link('OTHER'), link('UNKNOWN'), link('OPAQUE')]) {
    assert.ok(documentReferenceErrors({ OPAQUE: body, OTHER: 'body' }).length);
  }
});

test('essence freshness compares source content with normalized newlines', () => {
  const digest = createHash('sha256').update('opaque\n').digest('hex');
  const rows = [{ source: { path: 'fixture.md', sha256: digest } }];
  assert.deepEqual(staleMapSources(rows, () => 'opaque\r\n'), []);
  assert.deepEqual(staleMapSources(rows, () => 'changed\n'), ['fixture.md']);
});
