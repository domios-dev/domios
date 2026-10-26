import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { parse } from 'yaml';
import { loadNativeMsvcEnvironment } from '../release-arm64-compile.mjs';

const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const compile = parse(readFileSync(new URL('../../.github/actions/arm64-compile/action.yml', import.meta.url), 'utf8'));

const publicConfig = ['DOMIOS_FIREBASE_API_KEY', 'DOMIOS_FIREBASE_PROJECT_ID', 'DOMIOS_FIREBASE_APP_ID',
  'DOMIOS_ACCOUNT_REDEEM_URL', 'DOMIOS_BILLING_FUNCTIONS_URL', 'DOMIOS_AMPLITUDE_API_KEY_PROD'].sort();

test('the shared arm64 compile reads no secret itself and never sees signing keys', () => {
  assert.doesNotMatch(JSON.stringify(compile), /secrets\.|SIGNING_PRIVATE_KEY|GH_RELEASE_TOKEN|PACKAGE_REPOS_TOKEN/);
  const validate = compile.runs.steps.find(step => step.name === 'Validate source, platform and public build configuration');
  for (const key of publicConfig) assert.match(validate.run, new RegExp(key));
  const build = compile.runs.steps.find(step => step.id === 'handoff');
  assert.equal(build.env.CARGO_BUILD_JOBS, '4');
  assert.equal(build.env.RUSTUP_TOOLCHAIN, '1.99.1-${{ inputs.target }}');
  assert.equal(build.env.SCCACHE_GHA_ENABLED, 'true');
  assert.match(build.run, /release-arm64-compile\.mjs/);
  assert.equal(compile.outputs.sha256.value, '${{ steps.handoff.outputs.sha256 }}');
  const script = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
  assert.match(script, /createUpdaterArtifacts.*false/);
  assert.match(script, /tauri bundle --no-binary-patching/);
  assert.match(script, /prepare_release_bundle_inputs bundle/);
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

test('one hosted arm64 job compiles, signs, smokes and keeps the existing receipts', () => {
  const release = workflow('release');
  for (const [name, target] of [['linux', 'aarch64-unknown-linux-gnu'], ['windows', 'aarch64-pc-windows-msvc']]) {
    const arm = workflow(`release-${name}-arm64`);
    assert.equal(release.jobs[`${name}-arm64`].uses, `./.github/workflows/release-${name}-arm64.yml`);
    assert.deepEqual(Object.keys(arm.jobs), ['build']);
    const job = arm.jobs.build;
    assert.equal(job.needs, undefined);
    assert.equal(job['runs-on'], name === 'linux' ? 'ubuntu-24.04-arm' : 'windows-11-arm');
    assert.equal(job.environment, 'release');
    for (const key of publicConfig) assert.equal(job.env[key], '$' + '{{ secrets.' + key + ' }}');
    assert.equal(job.env.AWS_ACCESS_KEY_ID, '${{ secrets.SCCACHE_AWS_ACCESS_KEY_ID }}');
    const step = job.steps.findIndex(step => step.uses === './.github/actions/arm64-compile');
    assert.ok(step >= 0);
    assert.equal(job.steps[step].with.target, target);
    // Signing keys appear only after the compile, in the steps that sign.
    assert.ok(job.steps.slice(0, step).every(step => !/SIGNING_PRIVATE_KEY/.test(JSON.stringify(step))));
    const commands = job.steps.map(step => step.run ?? '').join('\n');
    assert.match(commands, /steps\.compile\.outputs\.sha256/);
    assert.match(commands, /signer sign/);
    assert.match(commands, /release-arm64-handoff\.mjs verify/);
    assert.match(commands, /release-bundle-smoke\.mjs/);
    assert.match(commands, /release-dry-run-manifest\.mjs capture/);
    assert.ok(job.steps.some(step => step.name === 'Retain the frozen stage proof'));
  }
  assert.match(workflow('release-windows').jobs.build.if, /inputs.arch == 'x64'/);
});
