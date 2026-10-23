import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

test('per-copy and per-request diagnostics require opt-in trace logging', () => {
  for (const [file, marker] of [
    ['codeterm-api/src/output_stream.rs', '[OUTPUT-COPY]'],
    ['codeterm-service-core/src/startup/rest_api/streams/diagnostics.rs', '[rest-request] accepted'],
  ]) {
    const source = readFileSync(new URL(`../../src-tauri/crates/${file}`, import.meta.url), 'utf8');
    const calls = [...source.matchAll(/log::(\w+)!\(\s*"([^"]*)"/g)]
      .filter(([, , message]) => message.startsWith(marker));
    assert.equal(calls.length, 1, file);
    assert.equal(calls[0][1], 'trace', file);
  }
});
