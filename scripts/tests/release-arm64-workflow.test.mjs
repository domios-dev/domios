import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { parse } from 'yaml';
import { loadNativeMsvcEnvironment } from '../release-arm64-compile.mjs';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const hosted = workflow('release-arm64-compile');

const publicConfig = ['DOMIOS_FIREBASE_API_KEY', 'DOMIOS_FIREBASE_PROJECT_ID', 'DOMIOS_FIREBASE_APP_ID',
  'DOMIOS_ACCOUNT_REDEEM_URL', 'DOMIOS_BILLING_FUNCTIONS_URL', 'DOMIOS_AMPLITUDE_API_KEY_PROD'].sort();
const cacheStore = ['SCCACHE_AWS_ACCESS_KEY_ID', 'SCCACHE_AWS_SECRET_ACCESS_KEY', 'CI_STORE_PUBLIC_ENDPOINT'];

test('hosted compilation receives the six public client settings and the cache store, no other secret', () => {
  assert.deepEqual(Object.keys(hosted.on.workflow_call.secrets).sort(), [...publicConfig, ...cacheStore].sort());
  const references = [...JSON.stringify(hosted).matchAll(/secrets\.([A-Z_]+)/g)].map(match => match[1]).sort();
  assert.deepEqual(references, [...publicConfig, ...cacheStore].sort());
  for (const key of cacheStore) assert.equal(hosted.on.workflow_call.secrets[key].required, false);
  for (const key of publicConfig) {
    assert.equal(hosted.on.workflow_call.secrets[key].required, true);
    assert.equal(hosted.jobs.compile.env[key], '$' + '{{ secrets.' + key + ' }}');
  }
  assert.doesNotMatch(JSON.stringify(hosted), /vars\.DOMIOS_/);
});

test('native MSVC setup preserves quoted batch paths and target arguments', { skip: process.platform !== 'win32' }, async () => {
  const directory = await mkdtemp(path.join(os.tmpdir(), 'arm64 toolchain '));
  try {
    const batch = path.join(directory, 'tool chain.bat');
    await writeFile(batch, '@echo off\r\nset CT_MSVC_FIXTURE=%1\r\n');
    const result = loadNativeMsvcEnvironment(batch, { ComSpec: process.env.ComSpec, SystemRoot: process.env.SystemRoot });
    assert.equal(result.CT_MSVC_FIXTURE, 'arm64');
  } finally { await rm(directory, { recursive: true, force: true }); }
});

test('hosted native arm64 compilation has read-only permissions and only cache keys', () => {
  const job = hosted.jobs.compile;
  assert.match(job['runs-on'], /windows-11-arm/);
  assert.match(job['runs-on'], /ubuntu-24.04-arm/);
  assert.deepEqual(hosted.permissions, { contents: 'read', actions: 'read' });
  assert.doesNotMatch(JSON.stringify(hosted), /self-hosted|tailscale|TAILNET|GH_RELEASE_TOKEN|PACKAGE_REPOS_TOKEN|SIGNING_PRIVATE_KEY/);
  assert.equal(job.env.SCCACHE_GHA_ENABLED, 'true');
  assert.equal(job.env.AWS_ACCESS_KEY_ID, '${{ secrets.SCCACHE_AWS_ACCESS_KEY_ID }}');
  assert.equal(job.env.CARGO_BUILD_JOBS, '4');
  assert.equal(job.env.RUSTUP_TOOLCHAIN, '1.99.1-${{ inputs.target }}');
  assert.ok(job['timeout-minutes'] > 0);
  assert.equal(job.steps.find(step => step.uses?.startsWith('actions/checkout')).with.ref, '${{ inputs.source_ref }}');
  const script = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
  assert.match(script, /createUpdaterArtifacts.*false/);
  assert.match(script, /tauri bundle --no-binary-patching/);
  assert.match(script, /prepare_release_bundle_inputs bundle/);

});

test('arm64 graph requires hosted handoff then hosted signing, verification and existing receipts', () => {
  const release = workflow('release');
  for (const [name, target] of [['linux', 'aarch64-unknown-linux-gnu'], ['windows', 'aarch64-pc-windows-msvc']]) {
    const arm = workflow(`release-${name}-arm64`);
    assert.equal(release.jobs[`${name}-arm64`].uses, `./.github/workflows/release-${name}-arm64.yml`);
    assert.equal(arm.jobs.compile.uses, './.github/workflows/release-arm64-compile.yml');
    assert.equal(arm.jobs.compile.with.target, target);
    assert.deepEqual(Object.keys(arm.jobs.compile.secrets).sort(), [...publicConfig, ...cacheStore].sort());
    for (const key of [...publicConfig, ...cacheStore]) assert.equal(arm.jobs.compile.secrets[key], '$' + '{{ secrets.' + key + ' }}');
    const final = arm.jobs.build;
    assert.equal(final.needs, 'compile');
    // Finalize runs on the hosted arm64 runner type that compiled, so the handoff never leaves GitHub.
    assert.equal(final['runs-on'], name === 'linux' ? 'ubuntu-24.04-arm' : 'windows-11-arm');
    assert.equal(final.environment, 'release');
    assert.equal(final.env.AWS_ACCESS_KEY_ID, '${{ secrets.SCCACHE_AWS_ACCESS_KEY_ID }}');
    assert.equal(final.env.AWS_SECRET_ACCESS_KEY, '${{ secrets.SCCACHE_AWS_SECRET_ACCESS_KEY }}');
    const commands = final.steps.map(step => step.run ?? '').join('\n');
    assert.match(commands, /needs\.compile\.outputs\.sha256/);
    assert.match(commands, /signer sign/);
    assert.match(commands, /release-arm64-handoff\.mjs verify/);
    assert.match(commands, /release-bundle-smoke\.mjs/);
    assert.match(commands, /release-dry-run-manifest\.mjs capture/);
    assert.doesNotMatch(commands, /cargo build|tauri build|release-bundle(?:\s|$)|release-inputs/);
    assert.ok(final.steps.some(step => step.name === 'Retain hosted handoff and final stage proof'));
  }
  assert.match(workflow('release-windows').jobs.build.if, /inputs.arch == 'x64'/);
});
