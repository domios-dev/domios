import assert from 'node:assert/strict';
import { readFileSync, readdirSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import test from 'node:test';
import { parse } from 'yaml';
const workflow = name => parse(readFileSync(new URL(`../../.github/workflows/${name}.yml`, import.meta.url), 'utf8'));
const release = workflow('release');
const macos = workflow('release-macos-build');
const linuxArm64 = workflow('release-linux-arm64');

test('dry-run consolidated manifests depend on all six builds and use only private CI transport', () => {
  const caller = release.jobs['dry-run-manifest'];
  assert.equal(caller.if, "${{ needs.prepare.outputs.dry_run == 'true' }}");
  for (const job of ['macos-arm64', 'macos-x64', 'linux-arm64', 'linux-x86_64', 'windows-x64', 'windows-arm64']) {
    assert.ok(caller.needs.includes(job));
  }
  const local = workflow('release-manifest-dry-run');
  assert.deepEqual(local.permissions, { contents: 'read', actions: 'read' });
  const text = readFileSync(new URL('../../.github/workflows/release-manifest-dry-run.yml', import.meta.url), 'utf8');
  assert.doesNotMatch(text, /gh release|git (?:push|tag)|PACKAGE_REPOS_TOKEN|GH_RELEASE_TOKEN/);
  assert.match(text, /release-dry-run-manifest\.mjs aggregate/);
  for (const name of ['release-linux', 'release-linux-arm64', 'release-windows', 'release-macos-build']) {
    const step = workflow(name).jobs.build.steps.find(step => step.name === 'Retain private dry-run updater evidence');
    assert.equal(step.if, '${{ inputs.dry_run }}');
    assert.match(step.run, /release-dry-run-manifest\.mjs capture/);
  }
});

test('Windows finalization reacquires the stable gateway mapping without compiling', () => {
  const step = workflow('release-windows').jobs.build.steps.find(step => step.name === 'Produce shared headless stage and package');
  assert.match(step.run, /build\.ps1 release-finalize/);
  const build = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
  assert.match(build, /release-bundle\|release-finalize\|release-preflight/);
  assert.match(build, /release-finalize\)\s+node scripts\/release-stage\.mjs finalize "\$\{@:2\}"\s+;;/);
});

test('Linux bundle-only step receives updater signing material after native compilation', () => {
  const steps = workflow('release-linux').jobs.build.steps;
  const bundle = steps.find(step => step.name === 'Stage final pair resources and bundle without compilation');
  assert.match(bundle.run, /pnpm tauri bundle --no-binary-patching/);
  assert.equal(bundle.env.TAURI_SIGNING_PRIVATE_KEY, '${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}');
  assert.equal(bundle.env.TAURI_SIGNING_PRIVATE_KEY_PASSWORD, '${{ secrets.TAURI_KEY_PASSWORD }}');
  assert.doesNotMatch(bundle.run, /echo.*TAURI_SIGNING|printenv/);
});
test('macOS and Linux arm64 have separate jobs, routing variables and manifest dependencies', () => {
  const callers = [
    ['macos-arm64', 'release-macos-build', 2, 'arm64'], ['macos-x64', 'release-macos-build', 2, 'x86_64'],
    ['linux-arm64', 'release-linux-arm64', 1, undefined],
  ];
  for (const [id, name, callerCount, arch] of callers) {
    const path = `./.github/workflows/${name}.yml`;
    assert.equal(release.jobs[id].uses, path);
    assert.equal(Object.values(release.jobs).filter(job => job.uses === path).length, callerCount);
    assert.equal(release.jobs[id].with.platform, undefined);
    assert.equal(release.jobs[id].with.arch, arch);
    assert.equal(release.jobs[id].secrets, 'inherit');
    const reusable = workflow(name);
    assert.deepEqual(Object.keys(reusable.on), name === 'release-macos-build'
      ? ['workflow_dispatch', 'workflow_call'] : ['workflow_call']);
    assert.deepEqual(Object.keys(reusable.on.workflow_call.inputs), ['tag', 'source_ref', 'mirror', 'dry_run', ...(arch ? ['arch'] : []), 'frontend_sha256']);
    assert.deepEqual(Object.keys(reusable.jobs), ['build']);
    assert.equal(reusable.jobs.build.if, undefined);
  }
  assert.equal(release.jobs['macos-arm64'].name, 'macOS arm64');
  assert.equal(release.jobs['macos-x64'].name, 'macOS x64');
  assert.equal(release.jobs['linux-arm64'].name, 'Linux arm64');
  assert.equal(macos.jobs.build.name, 'build');
  assert.equal(linuxArm64.jobs.build.name, 'build');
  assert.ok(release.jobs['release-manifest'].needs.includes('macos-arm64'));
  assert.ok(release.jobs['release-manifest'].needs.includes('macos-x64'));
  assert.ok(release.jobs['release-manifest'].needs.includes('linux-arm64'));
  assert.ok(release.jobs.prepare.steps.some(step => /ci-qualification/.test(step.run ?? '')));
  assert.ok(macos.jobs.build['runs-on'].some(label => label.includes('matrix.runner_label')));
  assert.match(linuxArm64.jobs.build['runs-on'][1], /RELEASE_LINUX_ARM64_LABEL.*codeterm-builder/);
  assert.equal(macos.jobs.build.strategy['fail-fast'], false);
  assert.deepEqual(macos.jobs.build.strategy.matrix.include.map(entry => entry.arch), ['${{ inputs.arch }}']);
  assert.deepEqual(['macos-arm64', 'macos-x64'].map(id => release.jobs[id].with.arch), ['arm64', 'x86_64']);
  for (const entry of macos.jobs.build.strategy.matrix.include) assert.match(entry.runner_label, /RELEASE_MACOS_.*_LABEL.*codeterm-builder/);
});
test('direct platform dispatch is scoped and cannot mutate release assets', () => {
  for (const name of ['release-linux', 'release-windows', 'release-macos-build']) {
    const platform = workflow(name);
    const inputs = platform.on.workflow_dispatch.inputs;
    assert.equal(inputs.source_ref.required, true, name);
    assert.equal(inputs.tag.required, true, name);
    assert.equal(inputs.dry_run.default, true, name);
    assert.equal(inputs.mirror.default, false, name);
    assert.deepEqual(Object.keys(platform.jobs), ['build'], name);
    for (const step of platform.jobs.build.steps) {
      if (/gh release upload|release-clear-wedged-assets|release-upload-assets|release-headless-assets\.mjs publish|release-publish-platform/.test(step.run || '') &&
          !step.env?.RELEASE_VERIFY_ONLY) {
        assert.match(step.if || '', /!\(github\.event_name == 'workflow_dispatch' && github\.workflow == '/, `${name}: ${step.name}`);
        assert.match(step.if, /!inputs\.dry_run/, `${name}: ${step.name}`);
        const condition = step.if.replace(/^\$\{\{\s*|\s*\}\}$/g, '');
        const evaluate = new Function('github', 'inputs', `return (${condition});`);
        const direct = { event_name: 'workflow_dispatch', workflow: platform.name };
        const caller = { event_name: 'workflow_dispatch', workflow: release.name };
        assert.equal(evaluate(direct, { dry_run: false, mirror: true }), false, `${name}: direct dispatch refuses publication`);
        assert.equal(evaluate(caller, { dry_run: false, mirror: true }), true, `${name}: manual release caller can publish`);
        assert.equal(evaluate(caller, { dry_run: true, mirror: true }), false, `${name}: manual release dry run refuses publication`);
      }
    }
  }
  assert.equal(workflow('tests').jobs.rust.if, "github.event_name == 'push' || github.event_name == 'schedule'", 'a manual tests dispatch runs the fast tier only');
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
const expression = text => new Function('github', 'inputs', 'vars', 'format',
  `return (${text.replace(/^\$\{\{\s*|\s*\}\}$/g, '')});`);
const format = (template, ...values) => template.replace(/\{(\d+)\}/g, (_, i) => values[i]);
test('a release-branch push stays a dry run of its own branch until publication is enabled', () => {
  const env = release.jobs.prepare.steps.find(step => step.id === 'release').env;
  const dryRun = expression(env.DRY_RUN);
  const group = expression(release.concurrency.group);
  const push = { event_name: 'push', ref_name: 'release/1.15.0' };
  const dispatch = { event_name: 'workflow_dispatch', ref_name: 'release/1.15.0' };
  const off = {};
  const on = { RELEASE_PUBLISH_ENABLED: 'true' };
  assert.equal(dryRun(push, {}, off, format), 'true');
  assert.equal(group(push, {}, off, format), 'release-dry-run-release/1.15.0');
  assert.equal(dryRun(push, {}, on, format), 'false');
  assert.equal(group(push, {}, on, format), 'release-publication');
  const dry = { dry_run: true, tag: 'v1.15.0', dry_run_tests: false };
  assert.equal(dryRun(dispatch, dry, on, format), 'true');
  assert.equal(group(dispatch, dry, on, format), 'release-dry-run-v1.15.0');
  for (const [id, job] of Object.entries(release.jobs)) assert.doesNotMatch(job.if || '', /inputs\.dry_run_tests/, id);
});
test('production publications serialize and verify draft bytes before becoming public', () => {
  assert.equal(release.concurrency['cancel-in-progress'], "${{ inputs.dry_run == true || vars.RELEASE_PUBLISH_ENABLED != 'true' }}");
  assert.match(release.concurrency.group, /release-publication/);
  const steps = workflow('release-manifest-validate').jobs.validate.steps;
  const verify = steps.findIndex(step => step.name === 'Verify draft artifact signatures before publication');
  const publish = steps.findIndex(step => step.name === 'Mark source and public releases published');
  assert.ok(verify >= 0 && verify < publish);
  assert.match(steps[verify].run, /--local/);
  assert.match(steps[verify].run, /gh release download/);
});
test('release CI actions are immutable and every executable job has a deadline', () => {
  for (const name of ['release', 'tests', 'release-macos-build', 'release-linux-arm64', 'release-linux', 'release-windows', 'release-manifest-validate', 'frontend-release', 'release-credentials', 'ready-for-release', 'qualification', 'precheck']) {
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
    for (const name of ['APPLE_SIGNING_IDENTITY', 'APPLE_TEAM_ID']) {
      assert.equal(job.env[name], '${{ secrets.' + name + ' }}');
    }
    const steps = job.steps;
    const setup = steps.findIndex(step => step.run === 'python3 scripts/release-signing-material.py macos');
    const cleanup = steps.findIndex(step => step.run === 'python3 scripts/release-signing-material.py cleanup');
    assert.ok(setup >= 0 && cleanup > setup);
    assert.equal(steps[cleanup].if, 'always()');
    for (const name of ['APPLE_CERTIFICATE', 'APPLE_CERTIFICATE_PASSWORD', 'APPLE_ID', 'APPLE_TEAM_ID', 'APPLE_PASSWORD', 'TAURI_SIGNING_PRIVATE_KEY']) {
      assert.equal(steps[setup].env[name], '${{ secrets.' + name + ' }}');
    }
    for (const [index, step] of steps.entries()) {
      if (/mac-bundle|signer sign|check-release-credentials-mac|release-notary-probe/.test(step.run || '')) assert.ok(setup < index && index < cleanup);
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
  for (const path of ['scripts/check-release-credentials-mac.sh', 'scripts/release-notary-probe.sh', 'scripts/release-macos-x86_64.sh', '.agents/skills/github-release/scripts/build-mac.sh', '.agents/skills/github-release/scripts/pack-mac-updater.sh', '.agents/skills/github-release/scripts/notarize-dmg.sh']) {
    assert.doesNotMatch(read(path), /\.tauri\/codeterm\.key|login\.keychain|--password/);
  }
  assert.match(read('scripts/check-release-credentials-mac.sh'), /release-signing-material\.py check-identity/);
  const releasePreparation = read('build.sh');
  assert.match(releasePreparation, /--keychain "\$RELEASE_KEYCHAIN"[\s\\]+--sign "\$APPLE_SIGNING_IDENTITY" src-tauri\/bundled-cli\/codeterm/);
  assert.ok(releasePreparation.indexOf('--sign "$APPLE_SIGNING_IDENTITY" src-tauri/bundled-cli/codeterm') <
    releasePreparation.indexOf('cargo build --release --manifest-path src-tauri/Cargo.toml -p domios-server'));
  assert.match(read('scripts/release-macos-x86_64.sh'), /CODETERM_RELEASE_COMMON_STAGE=1/);
});

test('packaging starts after preparation and frontend, with no tier polling or repeated suites', () => {
  for (const build of ['macos-arm64', 'macos-x64', 'linux-arm64', 'linux-x86_64', 'windows-x64', 'windows-arm64']) {
    assert.deepEqual(release.jobs[build].needs, ['prepare', 'frontend-dist']);
  }
  for (const id of ['tests-tier', 'builds-tier', 'quick-checks', 'tests']) assert.equal(release.jobs[id], undefined);
  assert.equal(release.jobs['frontend-dist'].needs, 'prepare');
});


test('native release processes share a job wrapper and verify before compiling', () => {
  for (const name of ['release-macos-build', 'release-linux', 'release-windows']) {
    const job = workflow(name).jobs.build;
    assert.equal(job.env.RUSTC_WRAPPER, 'sccache');
    const commands = job.steps.map(step => step.run || '');
    const verify = commands.findIndex(command => /build\.(?:sh|ps1) cache-verify/.test(command));
    const compile = commands.findIndex(command => /mac-bundle|build\.(?:sh|ps1) release-(?:inputs|bundle)/.test(command));
    assert.ok(verify >= 0 && compile > verify, name);
    const stats = job.steps.at(-3);
    assert.equal(stats.if, "${{ always() && steps.compile.outcome != 'skipped' }}");
    assert.equal(job.steps[compile].id, 'compile', name);
    assert.match(stats.run, /build\.(?:sh|ps1) cache-stats/);
    const release = job.steps.at(-2);
    assert.equal(release.if, 'always()', name);
    assert.match(release.run, /build\.(?:sh|ps1) cache-release/);
    if (name === 'release-windows') {
      assert.equal(stats.shell, 'pwsh');
      assert.ok(job.steps.every(step => step.shell !== 'bash'));
    }
  }
});

test('Windows Rust tests invoke the owning build forwarder without a bash shell', () => {
  const steps = Object.values(workflow('tests-rust').jobs).flatMap(job => job.steps || []);
  const archive = steps.find(step => step.run?.includes('build.ps1 ci-tests'));
  assert.ok(archive);
  assert.equal(archive.shell, 'pwsh');
  assert.equal(archive.if, "runner.os == 'Windows'");
  const unixArchive = steps.find(step => step.run?.includes('bash build.sh ci-tests ${{ matrix.suite }}'));
  assert.equal(unixArchive.if, "runner.os != 'Windows'");
});

test('compact unwind workaround belongs only to macOS platform tests, preserving release link flags', () => {
  const tests = workflow('tests-rust');
  const step = tests.jobs['rust-platform'].steps.find(step => step.run?.includes('bash build.sh ci-tests'));
  assert.match(step.run, /\[ "\$RUNNER_OS" = macOS \] && \[ "\$\{\{ matrix.suite \}\}" = platform \]/);
  for (const triple of ['AARCH64', 'X86_64']) {
    const name = `CARGO_TARGET_${triple}_APPLE_DARWIN_RUSTFLAGS`;
    assert.ok(step.run.includes(`export ${name}="\${${name}:+$${name} }-C link-arg=-Wl,-no_compact_unwind"`));
  }
  for (const name of ['release', 'release-macos-build', 'release-linux-arm64', 'release-linux', 'release-windows']) {
    assert.doesNotMatch(JSON.stringify(workflow(name)), /no_compact_unwind|CARGO_TARGET_.*_RUSTFLAGS/);
  }
  const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
  for (const [os, suite, expected] of [['macOS', 'platform', true], ['Linux', 'platform', false], ['macOS', 'full', false]]) {
    const preparation = step.run.split('bash build.sh')[0].replaceAll('${{ matrix.suite }}', suite);
    const flags = execFileSync(bash, ['-c', preparation + '\nprintf "%s" "$CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS"'],
      { encoding: 'utf8', env: { ...process.env, RUNNER_OS: os, CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS: '-C debuginfo=1' } });
    assert.equal(flags, '-C debuginfo=1' + (expected ? ' -C link-arg=-Wl,-no_compact_unwind' : ''));
  }
});

test('Docker forwards credential names and captures its own server counters', () => {
  const command = linuxArm64.jobs.build.steps.find(step => step.run?.includes('docker run --rm --pull=never --platform linux/arm64 \\')).run;
  assert.match(command, /-e AWS_ACCESS_KEY_ID -e AWS_SECRET_ACCESS_KEY/);
  assert.doesNotMatch(command, /-e AWS_ACCESS_KEY_ID=|--env-file/);
  const script = readFileSync(new URL('../../.codeterm/build-machine/scripts/build-linux.sh', import.meta.url), 'utf8');
  assert.match(script, /CODETERM_SCCACHE_VERIFY_ONLY=0 ct_configure_sccache/);
  assert.match(script, /trap 'ct_sccache_stats' EXIT/);
});
test('release builds keep bundle verification while server and CLI runtime checks belong to the test tier', () => {
  const directory = new URL('../../.github/workflows/', import.meta.url);
  for (const file of readdirSync(directory).filter(name => /^release.*\.yml$/.test(name))) {
    const source = readFileSync(new URL(file, directory), 'utf8');
    assert.doesNotMatch(source, /release-pair-smoke|Smoke packaged client and isolated server pair/, file);
  }
  const container = readFileSync(new URL('../../.codeterm/build-machine/scripts/build-linux.sh', import.meta.url), 'utf8');
  assert.doesNotMatch(container, /release-pair-smoke/);
  assert.match(container, /release-bundle-smoke\.mjs/);
  for (const name of ['release-linux', 'release-macos-build', 'release-windows']) {
    const steps = workflow(name).jobs.build.steps;
    const commands = steps.map(step => step.run || '').join('\n');
    assert.match(commands, /release-bundle-smoke\.mjs/);
    const signature = steps.find(step => /^Verify.*signature/.test(step.name || ''));
    assert.ok(signature, `${name}: signing verification remains required`);
    assert.equal(signature.if, undefined, `${name}: signature check also runs in dry runs`);
    assert.match(signature.run, /verify-updater-manifest|release-verify-windows|release-publish-platform/);
  }
});
