import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const release = workflow('release');
const macos = workflow('release-macos-build');
const linuxArm64 = workflow('release-linux-arm64');
test('macOS and Linux arm64 have separate jobs, routing variables and manifest dependencies', () => {
  for (const [id, name] of [['macos', 'release-macos-build'], ['linux-arm64', 'release-linux-arm64']]) {
    const path = `./.github/workflows/${name}.yml`;
    assert.equal(release.jobs[id].uses, path);
    assert.equal(Object.values(release.jobs).filter(job => job.uses === path).length, 1);
    assert.equal(release.jobs[id].with.platform, undefined);
    assert.equal(release.jobs[id].secrets, 'inherit');
    const reusable = workflow(name);
    assert.deepEqual(Object.keys(reusable.on), ['workflow_call']);
    assert.deepEqual(Object.keys(reusable.on.workflow_call.inputs), ['tag', 'source_ref', 'mirror', 'dry_run']);
    assert.deepEqual(Object.keys(reusable.jobs), ['build']);
    assert.equal(reusable.jobs.build.if, undefined);
  }
  assert.equal(release.jobs.macos.name, 'macOS');
  assert.equal(release.jobs['linux-arm64'].name, 'Linux arm64');
  assert.equal(macos.jobs.build.name, 'build (${{ matrix.arch }})');
  assert.equal(linuxArm64.jobs.build.name, 'build');
  assert.ok(release.jobs['release-manifest'].needs.includes('macos'));
  assert.ok(release.jobs['release-manifest'].needs.includes('linux-arm64'));
  assert.ok(release.jobs['release-manifest'].needs.includes('tests'));
  assert.ok(macos.jobs.build['runs-on'].some(label => label.includes('matrix.runner_label')));
  assert.match(linuxArm64.jobs.build['runs-on'][1], /RELEASE_LINUX_ARM64_LABEL.*codeterm-builder/);
  assert.equal(macos.jobs.build.strategy['fail-fast'], false);
  assert.deepEqual(macos.jobs.build.strategy.matrix.include.map(entry => entry.arch), ['arm64', 'x86_64']);
  for (const entry of macos.jobs.build.strategy.matrix.include) assert.match(entry.runner_label, /RELEASE_MACOS_.*_LABEL.*codeterm-builder/);
});
test('every platform checks its own built bundle before any upload, with independent Windows retries', () => {
  for (const name of ['release-macos-build', 'release-linux-arm64', 'release-linux', 'release-windows']) {
    for (const job of Object.values(workflow(name).jobs)) {
      const commands = job.steps.map(step => step.run || '');
      const smoke = commands.findIndex(command => /release-bundle-smoke|bash \/build-linux.sh source/.test(command));
      assert.ok(smoke >= 0, name);
      const uploads = job.steps.map((step, i) => /gh release upload|release-publish-platform/.test(step.run || '') && !step.env?.RELEASE_VERIFY_ONLY ? i : -1).filter(i => i >= 0);
      for (const upload of uploads) assert.ok(smoke < upload, `${name}: smoke before upload`);
      for (const step of job.steps) assert.equal(step.with?.tagName, undefined, 'tauri-action builds only');
      assert.ok(job['timeout-minutes'] > 0);
      assert.equal(job.env.CODETERM_SCCACHE_VERIFY_ONLY, '1');
    }
  }
  assert.equal(workflow('release-windows').jobs.build.strategy['fail-fast'], false);
});
test('production publications serialize and verify draft bytes before becoming public', () => {
  assert.equal(release.concurrency['cancel-in-progress'], '${{ inputs.dry_run == true }}');
  assert.match(release.concurrency.group, /release-publication/);
  const steps = workflow('release-manifest-validate').jobs.validate.steps;
  const verify = steps.findIndex(step => step.name === 'Verify draft artifact signatures before publication');
  const publish = steps.findIndex(step => step.name === 'Mark source and public releases published');
  assert.ok(verify >= 0 && verify < publish);
  assert.match(steps[verify].run, /--local/);
  assert.match(steps[verify].run, /gh release download/);
});
test('release CI actions are immutable and every executable job has a deadline', () => {
  for (const name of ['release', 'tests', 'release-macos-build', 'release-linux-arm64', 'release-linux', 'release-windows', 'release-manifest-validate', 'frontend-release', 'release-credentials', 'ready-for-release', 'qualification', 'precheck', 'build-gateway']) {
    for (const job of Object.values(workflow(name).jobs)) {
      if (job.uses) continue;
      assert.ok(job['timeout-minutes'] > 0, `${name} deadline`);
      for (const step of job.steps) {
        if (step.uses && !step.uses.startsWith('./')) assert.match(step.uses, /@[0-9a-f]{40}$/);
        assert.doesNotMatch(step.run || '', /\bsleep\s+\d/);
      }
    }
  }
});

test('Mac jobs provision secret material before building and always clean up', () => {
  for (const job of [macos.jobs.build, linuxArm64.jobs.build, workflow('release-credentials').jobs.macos]) {
    const steps = job.steps;
    const setup = steps.findIndex(step => step.run === 'python3 scripts/release-signing-material.py macos');
    const cleanup = steps.findIndex(step => step.run === 'python3 scripts/release-signing-material.py cleanup');
    assert.ok(setup >= 0 && cleanup > setup);
    assert.equal(steps[cleanup].if, 'always()');
    for (const name of ['APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_ID', 'APPLE_TEAM_ID', 'APPLE_PASSWORD', 'TAURI_SIGNING_PRIVATE_KEY']) {
      assert.equal(steps[setup].env[name], '${{ secrets.' + name + ' }}');
    }
    for (const [index, step] of steps.entries()) {
      if (/mac-bundle|signer sign|check-release-credentials-mac/.test(step.run || '')) assert.ok(setup < index && index < cleanup);
      assert.doesNotMatch(step.run || '', /\.tauri\/codeterm\.key|login\.keychain/);
    }
  }
  assert.equal(macos.jobs.build.env.APPLE_SIGNING_IDENTITY, '${{ secrets.APPLE_SIGNING_IDENTITY }}');
  for (const job of [macos.jobs.build, linuxArm64.jobs.build]) assert.equal(job.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD, '${{ secrets.TAURI_KEY_PASSWORD }}');
  const read = path => readFileSync(new URL(`../../${path}`, import.meta.url), 'utf8');
  const provision = read('scripts/release-signing-material.py');
  for (const command of ['create-keychain', 'import', 'set-key-partition-list', 'list-keychains', 'delete-keychain']) assert.ok(provision.includes(`'${command}'`));
  assert.match(provision, /required\('RUNNER_TEMP'\)/);
  assert.match(provision, /chmod\(0o600\)/);
  for (const path of ['scripts/check-release-credentials-mac.sh', 'scripts/release-macos-x86_64.sh', '.agents/skills/github-release/scripts/build-mac.sh', '.agents/skills/github-release/scripts/pack-mac-updater.sh', '.agents/skills/github-release/scripts/notarize-dmg.sh']) {
    assert.doesNotMatch(read(path), /\.tauri\/codeterm\.key|login\.keychain|--password/);
  }
  assert.match(read('scripts/check-release-credentials-mac.sh'), /release-signing-material\.py check-identity/);
  assert.match(read('scripts/release-macos-x86_64.sh'), /--keychain "\$RELEASE_KEYCHAIN" --sign "\$SIGN_ID"/);
});

test('platform builds wait only for fast checks while Rust tests queue earlier', () => {
  for (const build of ['macos', 'linux-arm64', 'linux-x86_64', 'windows']) {
    assert.deepEqual(release.jobs[build].needs, ['prepare', 'quick-checks']);
  }
  assert.equal(release.jobs.tests.needs, 'prepare');
  assert.equal(release.jobs.tests.with.phase, 'long');
  assert.ok(release.jobs['release-manifest'].needs.includes('quick-checks'));
});
