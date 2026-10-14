import assert from 'node:assert/strict';
import test from 'node:test';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { archivePlatform, archiveRunArgs, archiveTargets, formatReleasePlan, generatedDirectories, loadReleaseRust, releaseRustPlan, validateArchiveSuite, verifyArchiveBuilds } from '../ci-rust-archives.mjs';

const builds = () => ['macOS', 'Linux', 'Windows'].map((os) => ({ os, arch: os === 'macOS' ? 'ARM64' : 'X64', sha: 'TOKEN_SHA', exit: 0, runner: os + '-builder', seconds: 10 }));
const receipts = () => builds().map(({ os, arch, sha }) => ({ os, arch, sha }));
test('generated source directories come from executed Cargo package messages', () => {
  const messages = [{reason: 'compiler-artifact', package_id: 'A'}, {reason: 'build-script-executed', package_id: 'A', out_dir: '/exact/out'}];
  assert.deepEqual(generatedDirectories(messages.map(JSON.stringify).join('\n')), { A: '/exact/out' });
});
test('archive identity binds operating system and architecture', () => {
  assert.equal(archivePlatform('macOS', 'ARM64'), 'macOS-ARM64');
  assert.throws(() => archivePlatform('Linux', 'unknown'));
});
test('classified archive inventory must match source OS and architecture', () => {
  const document = { os: 'Windows', arch: 'X64', sha: 'TOKEN_SHA', rows: [{ id: 'test' }] };
  assert.deepEqual(validateArchiveSuite(document, 'Windows', 'X64', 'TOKEN_SHA'), document.rows);
  for (const expected of [['Linux', 'X64', 'TOKEN_SHA'], ['Windows', 'ARM64', 'TOKEN_SHA'], ['Windows', 'X64', 'OTHER_SHA']]) assert.throws(() => validateArchiveSuite(document, ...expected));
  assert.throws(() => validateArchiveSuite({ ...document, rows: [] }, 'Windows', 'X64', 'TOKEN_SHA'));
});
test('all consumers reference one successful archive build per OS and architecture', () => {
  assert.match(verifyArchiveBuilds(builds(), receipts()), /macOS-builder.*10.0 s/);
  const failed = builds(); failed[0].exit = 1;
  assert.throws(() => verifyArchiveBuilds(failed, receipts()), /failed/);
  assert.throws(() => verifyArchiveBuilds(builds().slice(1), receipts()), /Missing/);
  assert.throws(() => verifyArchiveBuilds([...builds(), builds()[0]], receipts()), /Duplicate/);
  const wrong = receipts(); wrong[0].sha = 'OTHER_SHA';
  assert.throws(() => verifyArchiveBuilds(builds(), wrong), /matching/);
});
test('archive runs contain remapping and partition runtime controls without compiler options', () => {
  const args = archiveRunArgs('ARCHIVE', 'SOURCE', 'EXTRACT', 'CONFIG', ['--partition', 'hash:1/12']);
  assert.equal(args[args.indexOf('--archive-file') + 1], 'ARCHIVE');
  assert.equal(args[args.indexOf('--workspace-remap') + 1], 'SOURCE');
  assert.ok(!args.some((arg) => ['--manifest-path', '--lib', '--workspace', '--features', '--locked'].includes(arg)));
  assert.ok(!args.includes('--test-threads'));
  assert.equal(archiveRunArgs('ARCHIVE', 'SOURCE', 'EXTRACT', 'CONFIG', [], true).at(-1), '1');
});
test('only archive builders install Rust; free consumers choose their platform artifact', () => {
  const workflow = readFileSync(new URL('../../.github/workflows/tests.yml', import.meta.url), 'utf8');
  const consumers = workflow.slice(workflow.indexOf('  rust-shards:'), workflow.indexOf('  rust-coverage:'));
  assert.doesNotMatch(consumers, /rust-toolchain|Prepare compile fixtures/);
  assert.match(consumers, /rust-archive-\$\{\{ runner.os \}\}-\$\{\{ runner.arch \}\}/);
  assert.match(consumers, /runs-on: \[self-hosted, rust-shard, macOS\]/);
});


test('macOS shared shards and all native consumers retain matching archive builds', () => {
  const native = receipts();
  const shared = Array.from({ length: 12 }, () => ({ ...native[0] }));
  assert.match(verifyArchiveBuilds(builds(), [...native, ...shared]), /Linux-builder/);
});

const packagesOf = (args) => args.flatMap((arg, index) => arg === '-p' ? [args[index + 1]] : []);
const PREVIOUSLY_SKIPPED = ['codeterm-input-macos', 'codeterm-input-windows', 'codeterm-analytics', 'codeterm-sentinel-ports', 'codeterm-browser',
  'codeterm-storage', 'codeterm-agent-detect', 'codeterm-notification-core', 'codeterm-repowatch', 'codeterm-usage', 'codeterm-blueprint-types',
  'codeterm-local-recording', 'codeterm-notify-types', 'codeterm-events', 'codeterm-config', 'codeterm-secrets', 'codeterm-keepawake',
  'codeterm-cron', 'codeterm-shutdown'];

test('the release Rust set comes from the catalogue release tier and leaves pty-fork manual', () => {
  const { runners, overlay, testCounts } = loadReleaseRust(fileURLToPath(new URL('../..', import.meta.url)));
  const released = runners.map((r) => r.package);
  for (const pkg of [...PREVIOUSLY_SKIPPED, 'codeterm-mem-store', 'codeterm-paths', 'codeterm', 'codeterm-service-core', 'codeterm-cli']) assert.ok(released.includes(pkg), pkg);
  assert.ok(!released.includes('portable-pty'));
  assert.ok(testCounts['rust-mem-store'] > 0 && testCounts['rust-paths'] > 0);
  const plan = releaseRustPlan(runners, overlay, 'linux', [], testCounts);
  assert.equal(plan.find((g) => g.ids.includes('rust-service-core')).noTests, 'fail');
  assert.equal(plan.flatMap((g) => g.ids).sort().join(), runners.map((r) => r.id).sort().join());
  assert.deepEqual(plan.find((g) => g.scope === 'input-platform').ids, ['rust-input-macos', 'rust-input-windows']);
  assert.equal(plan.find((g) => g.ids.includes('rust-service-core')).scope, 'app');
  const workspace = archiveTargets('workspace', runners);
  const archived = packagesOf(workspace);
  for (const pkg of PREVIOUSLY_SKIPPED.slice(2)) assert.ok(archived.includes(pkg), pkg);
  for (const pkg of ['codeterm-mem-store', 'codeterm-paths', 'codeterm']) assert.ok(archived.includes(pkg), pkg);
  assert.ok(!archived.includes('portable-pty'));
  assert.deepEqual(workspace.slice(-3), ['--lib', '--test', 'main']);
  assert.deepEqual(archiveTargets('cli', runners), ['-p', 'codeterm-cli', '--all-targets']);
});

const runner = (id, extra = {}) => ({ id, lang: 'rust', kind: 'unit', package: id, manifest: 'src-tauri/Cargo.toml', runner: './build.sh nextest ' + id, budget_secs: 600, ci: ['tests.yml:rust-shards'], ...extra });

test('native recording packages keep their own CI job outside the shared archive', () => {
  const runners = [runner('codeterm-a'), runner('codeterm-input-macos', { ci: ['tests.yml:rust-input-platform'] })];
  assert.deepEqual(archiveTargets('workspace', runners), ['-p', 'codeterm-a', '--lib']);
});

test('archive target flags are the union over the lane and keep each --test name paired', () => {
  const runners = [runner('a', { cargo_targets: ['--lib', '--test', 'main'] }), runner('b', { cargo_targets: ['--lib', '--test', 'other'] }), runner('c')];
  assert.deepEqual(archiveTargets('workspace', runners), ['-p', 'a', '-p', 'b', '-p', 'c', '--lib', '--test', 'main', '--test', 'other']);
  assert.throws(() => archiveTargets('cli', runners), /No release Rust runner/);
  assert.throws(() => archiveTargets('workspace', [runner('d', { ci: undefined })]), /no CI placement: d/);
});

test('release plan refuses a runner outside build.sh nextest and an unknown selection', () => {
  const overlay = { stale: [], flaky: [], platform_red: [] };
  assert.throws(() => releaseRustPlan([runner('a', { runner: 'cargo nextest run -p a' })], overlay), /does not run through \.\/build\.sh nextest/);
  assert.throws(() => releaseRustPlan([runner('a')], overlay, 'linux', ['b']), /Not release-tier Rust runners: b/);
  assert.deepEqual(releaseRustPlan([runner('a'), runner('b')], overlay, 'linux', ['b']).map((g) => g.ids), [['b']]);
});

test('only a group whose whole inventory is empty passes the no-tests check', () => {
  const overlay = { stale: [], flaky: [], platform_red: [] };
  const shared = { runner: './build.sh nextest shared' };
  const plan = releaseRustPlan([runner('a'), runner('b', shared), runner('c', shared), runner('d')], overlay, 'linux', [], { a: 0, b: 0, c: 3 });
  assert.deepEqual(plan.map((g) => [g.scope, g.noTests]), [['a', 'pass'], ['shared', 'fail'], ['d', 'fail']]);
});

test('stale and this platform\'s platform-red Rust markers become nextest exclusions; flaky ones still run', () => {
  const marker = (test, extra = {}) => ({ runner: 'a', target: { binary: 'a', test }, reason: 'TOKEN_REASON', source: { task: 'task_x' }, ...extra });
  const overlay = {
    stale: [marker('m::stale')],
    flaky: [marker('m::flaky')],
    platform_red: [marker('m::linux_red', { platforms: ['linux'], owner: 'TOKEN_OWNER' }), marker('m::mac_red', { platforms: ['macos'], owner: 'TOKEN_OWNER' })],
  };
  const [linux] = releaseRustPlan([runner('a')], overlay, 'linux');
  assert.equal(linux.filterset, 'not (binary_id(=a) & test(=m::stale)) & not (binary_id(=a) & test(=m::linux_red))');
  assert.equal(formatReleasePlan([linux]), 'a\ta\t600\tfail\t' + linux.filterset + '\n');
  assert.throws(() => releaseRustPlan([runner('a')], { ...overlay, stale: [marker('m::x) | all(')] }, 'linux'), /Unsafe overlay target/);
});
