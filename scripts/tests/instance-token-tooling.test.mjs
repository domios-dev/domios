import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, mkdirSync, readFileSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';

test('DEV clean restart obtains only the selected installed CLI credential and targets its port', () => {
  const root = mkdtempSync(path.join(tmpdir(), 'vault-tooling-'));
  const data = path.join(root, 'dev');
  const bin = path.join(data, 'bin');
  const evidence = path.join(root, 'calls');
  mkdirSync(bin, { recursive: true });
  try {
    const body = readFileSync('build.sh', 'utf8').match(/^request_dev_clean_restart\(\) \{[\s\S]*?^\}/m)?.[0];
    assert.ok(body);
    writeFileSync(path.join(bin, 'codeterm'), `#!/usr/bin/env bash
[ "$1 $2" = 'system api-token' ] || exit 1
[ "$CODETERM_DATA_DIR" = "$CT_DATA_DIR" ] || exit 2
[ -z "\${CODETERM_API_TOKEN:-}" ] || exit 3
[ -z "\${CODETERM_API_URL:-}" ] || exit 4
printf '%s' selected-credential
`, { mode: 0o700 });
    writeFileSync(path.join(bin, 'curl'), `#!/usr/bin/env bash
printf '%s\\n' "$@" > "$TOKEN_TRACE"
`, { mode: 0o700 });
    const result = spawnSync(bash, ['-c', `curl() { "$TOKEN_TEST_BIN/curl" "$@"; }\n${body}\nrequest_dev_clean_restart`], {
      encoding: 'utf8', windowsHide: true,
      env: { ...process.env, PATH: `${bin}${path.delimiter}${process.env.PATH}`,
        TOKEN_TEST_BIN: bin.replaceAll('\\', '/'), CT_DATA_DIR: data.replaceAll('\\', '/'), CT_PORT: '7799', TOKEN_TRACE: evidence.replaceAll('\\', '/'),
        CODETERM_API_TOKEN: 'inherited-production', CODETERM_API_URL: 'http://127.0.0.1:7685' },
    });
    assert.equal(result.status, 0, result.stderr);
    assert.equal(result.stdout, '');
    const calls = readFileSync(evidence, 'utf8');
    assert.match(calls, /Authorization: Bearer selected-credential/);
    assert.match(calls, /http:\/\/127\.0\.0\.1:7799\/api\/debug\/shutdown\/clean-restart/);
    assert.doesNotMatch(calls, /7685|inherited-production/);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
