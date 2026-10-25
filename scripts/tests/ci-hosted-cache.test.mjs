import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
test('GitHub sccache storage clears S3 credentials and does not read the vault', () => {
  const source = readFileSync(new URL('../cache-policy.sh', import.meta.url), 'utf8');
  const functions = source.slice(source.indexOf('ct_sccache_storage() {'), source.indexOf('ct_sccache_disk_dir() {')) +
    source.slice(source.indexOf('ct_sccache_storage_matches() {'), source.indexOf('ct_sccache_restart() {'));
  const script = functions + '\nct_sccache_secret() { echo VAULT_CALLED; return 1; }\n' +
    'SCCACHE_GHA_ENABLED=true\nAWS_ACCESS_KEY_ID=private\nAWS_SECRET_ACCESS_KEY=private\n' +
    'ct_sccache_storage\n[ "$CT_SCCACHE_STORAGE" = gha ]\n' +
    '[ -z "$AWS_ACCESS_KEY_ID$AWS_SECRET_ACCESS_KEY$SCCACHE_ENDPOINT" ]\n' +
    "ct_sccache_storage_matches 'ghac, name: fixture'\n" +
    "! ct_sccache_storage_matches 's3, name: fixture'\n";
  const result = spawnSync(bash, ['-e', '-c', script], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.doesNotMatch(result.stdout, /VAULT_CALLED/);
});
