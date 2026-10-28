import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { tmpdir } from 'node:os';
import { parseEnv } from 'node:util';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { buildSecretNames, createLocalBuildEnv, localBuildEnvPath, readLocalBuildEnv } from '../local-build-env.mjs';

const repo = fileURLToPath(new URL('../..', import.meta.url));
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const values = Object.fromEntries(Object.keys(buildSecretNames).map(key => [key, `opaque-${key}`]));
const localEnv = { ...process.env, CI: '' };
function fixture(t) {
  const directory = fs.mkdtempSync(path.join(tmpdir(), 'local-build-env-'));
  t.after(() => fs.rmSync(directory, { recursive: true, force: true }));
  function git(args) {
    const result = spawnSync('git', args, { cwd: directory, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  }
  git(['init', '-q']);
  fs.writeFileSync(path.join(directory, '.gitignore'), '.env\n.env.*\n');
  fs.writeFileSync(path.join(directory, '.env'), Object.entries(values).map(([key, value]) => `${key}=${JSON.stringify(value)}`).join('\n'));
  return { directory, git, file: path.join(directory, '.env') };
}

test('worktrees read one ignored main-checkout file without copying it', t => {
  const { directory, git, file } = fixture(t);
  git(['add', '.gitignore']);
  git(['-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', '-c', 'core.hooksPath=/dev/null', 'commit', '-qm', 'fixture']);
  const lane = path.join(directory, 'lane');
  git(['worktree', 'add', '--detach', lane, 'HEAD']);
  assert.equal(localBuildEnvPath(lane), file);
  assert.deepEqual(readLocalBuildEnv(lane), values);
  assert.equal(fs.existsSync(path.join(lane, '.env')), false);
});

test('missing keys report their name without values', t => {
  const { directory, file } = fixture(t);
  fs.writeFileSync(file, 'DOMIOS_FIREBASE_API_KEY="opaque-do-not-log"\n');
  assert.throws(() => readLocalBuildEnv(directory), error => {
    assert.match(error.message, /DOMIOS_FIREBASE_PROJECT_ID/);
    assert.doesNotMatch(error.message, /opaque/);
    return true;
  });
});

test('tracked or nonignored environment files are refused', t => {
  const { directory, git } = fixture(t);
  git(['add', '-f', '.env']);
  assert.throws(() => readLocalBuildEnv(directory), /must be ignored/);
});

test('one-time vault refresh preserves unrelated entries and restricts access', { timeout: 20_000 }, async t => {
  const { directory, file } = fixture(t);
  fs.appendFileSync(file, '\nUNRELATED_KEY="opaque-retained"\n');
  const names = [];
  await createLocalBuildEnv({ cwd: directory, env: localEnv, readVault: async (_cli, name) => {
    names.push(name);
    return `opaque-refreshed-${name}`;
  } });
  assert.deepEqual(names.sort(), Object.values(buildSecretNames).sort());
  assert.equal(parseEnv(fs.readFileSync(file, 'utf8')).UNRELATED_KEY, 'opaque-retained');
  assert.equal(readLocalBuildEnv(directory).DOMIOS_FIREBASE_API_KEY, 'opaque-refreshed-DOMIOS_FIREBASE_API_KEY');
  if (process.platform === 'win32') {
    const result = spawnSync('pwsh', ['-NoProfile', '-NonInteractive', '-Command',
      '$acl=Get-Acl -LiteralPath $env:CODETERM_ENV_ACL_PATH; $sid=[System.Security.Principal.WindowsIdentity]::GetCurrent().User.Value; '
      + 'if(!$acl.AreAccessRulesProtected -or @($acl.Access).Count -ne 1 -or $acl.Access[0].IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value -ne $sid){exit 9}'],
    { env: { ...localEnv, CODETERM_ENV_ACL_PATH: file }, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
  } else assert.equal(fs.statSync(file).mode & 0o777, 0o600);
});

test('CI creation is refused before reading the vault', async t => {
  const { directory } = fixture(t);
  await assert.rejects(createLocalBuildEnv({ cwd: directory, env: { CI: 'true' }, readVault: () => {
    assert.fail('CI must retain its own secure source');
  } }), /unavailable in CI/);
});

test('DEV consumers use local values without vault calls; release and CI keep their source', t => {
  const { directory } = fixture(t);
  fs.mkdirSync(path.join(directory, 'scripts'));
  for (const name of ['local-build-env.mjs', 'build-identity.cjs']) {
    fs.copyFileSync(path.join(repo, 'scripts', name), path.join(directory, 'scripts', name));
  }
  const cache = fs.readFileSync(path.join(repo, 'scripts/cache-policy.sh'), 'utf8').match(/^ct_sccache_secret\(\) \([^]*?^\)/m)[0];
  const result = spawnSync(bash, ['-s', '--', repo.replaceAll('\\', '/')], { cwd: directory, env: localEnv, encoding: 'utf8', input: `set -euo pipefail
source "$1/scripts/local-build-env.sh"
source "$1/scripts/firebase-build-config.sh"
${cache}
CMD=dev DEV_NOTIFY_PARENT_CLI=
unset ${Object.keys(buildSecretNames).join(' ')}
codeterm() { printf vault-used >&2; return 91; }
ct_load_local_build_env
ct_configure_dev_qa_project
[ "$DOMIOS_QA_PROJECT_ID" = opaque-DOMIOS_QA_PROJECT_ID ]
[ -z "\${DOMIOS_FIREBASE_API_KEY+x}" ]
consumer() {
  [ "$DOMIOS_FIREBASE_API_KEY" = opaque-DOMIOS_FIREBASE_API_KEY ]
  [ "$DOMIOS_QA_FIREBASE_API_KEY" = opaque-DOMIOS_QA_FIREBASE_API_KEY ]
}
ct_with_firebase_build_config consumer
[ -z "\${DOMIOS_FIREBASE_API_KEY+x}" ]
[ "$(ct_sccache_secret domios-sccache-s3-access-key)" = opaque-DOMIOS_SCCACHE_S3_ACCESS_KEY ]
[ "$(ct_sccache_secret domios-sccache-s3-secret-key)" = opaque-DOMIOS_SCCACHE_S3_SECRET_KEY ]
[ "$(ct_sccache_secret domios-ci-store-endpoint)" = opaque-DOMIOS_CI_STORE_ENDPOINT ]
unset CT_LOCAL_BUILD_ENV_DOMIOS_SCCACHE_S3_SECRET_KEY
status=0
ct_sccache_secret domios-sccache-s3-secret-key 2>missing.log || status=$?
[ "$status" = 2 ]
grep -q DOMIOS_SCCACHE_S3_SECRET_KEY missing.log
CMD=release
status=0
ct_firebase_build_secret DOMIOS_FIREBASE_API_KEY 2>/dev/null || status=$?
[ "$status" = 91 ]
CMD=dev CI=true
! ct_uses_local_build_env
status=0
ct_firebase_build_secret DOMIOS_FIREBASE_API_KEY 2>/dev/null || status=$?
[ "$status" = 91 ]
printf consumer-verified
` });
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, /consumer-verified/);
  assert.doesNotMatch(result.stderr, /vault-used|opaque/);
});
