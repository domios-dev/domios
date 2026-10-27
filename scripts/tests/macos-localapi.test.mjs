import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';

const model = readFileSync(new URL('../../src-tauri/crates/codeterm-mesh/src/serve_setup/macos_localapi_model.sh', import.meta.url), 'utf8');
const shell = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : '/bin/sh';

function run(expression, ...inputs) {
  const result = spawnSync(shell, ['-c', `${model}\n${expression}`, 'fixture', ...inputs], { encoding: 'utf8' });
  assert.ifError(result.error);
  return result;
}

function config(result) {
  assert.equal(result.status, 0, result.stderr);
  return Object.fromEntries(result.stdout.trim().split('\n').map(line => {
    const match = /^(\w+) = "([^"]*)"$/.exec(line);
    assert.ok(match);
    return [match[1], match[2]];
  }));
}

test('macsys without an App Store proof uses the system port and proof contents', () => {
  const request = config(run('request_config "$@"', '12345', 'a1b2c3', '/localapi/v0/status'));
  const url = new URL(request.url);
  assert.equal(url.hostname, '127.0.0.1');
  assert.equal(url.port, '12345');
  assert.equal(url.pathname, '/localapi/v0/status');
  assert.equal(request.user, ':a1b2c3');
});

test('LocalAPI request encoding names the listener port in Host', () => {
  const request = config(run('request_config "$@"', '12345', 'a1b2c3', '/localapi/v0/serve-config'));
  assert.deepEqual(Buffer.from(request.header), Buffer.from('Host: 127.0.0.1:12345'));
});

test('App Store proof filenames keep their port and credential', () => {
  const request = config(run('app_store_request_config "$@"', 'sameuserproof-23456-abcdef', '/localapi/v0/status'));
  assert.equal(new URL(request.url).port, '23456');
  assert.equal(request.user, ':abcdef');
});

test('malformed App Store proof allows the system extension fallback', () => {
  const request = config(run('app_store_request_config "$1" "$2" || request_config "$3" "$4" "$2"',
    'sameuserproof-12345', '/localapi/v0/status', '23456', 'abcdef'));
  assert.equal(new URL(request.url).port, '23456');
  assert.equal(request.user, ':abcdef');
});

test('request model rejects missing, overflowing, and injected credential inputs', () => {
  for (const [port, proof, endpoint] of [
    ['', 'abc123', '/localapi/v0/status'],
    ['0', 'abc123', '/localapi/v0/status'],
    ['65536', 'abc123', '/localapi/v0/status'],
    ['99999999999999999999999', 'abc123', '/localapi/v0/status'],
    ['../12345', 'abc123', '/localapi/v0/status'],
    ['12345', '', '/localapi/v0/status'],
    ['12345', 'abc\r\nInjected', '/localapi/v0/status'],
    ['12345', 'abc123', '/localapi/v0/serve-config?write=true'],
  ]) {
    const result = run('request_config "$@"', port, proof, endpoint);
    assert.notEqual(result.status, 0);
    assert.equal(result.stdout, '');
  }
});
