import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { execFileSync, spawnSync } from 'node:child_process';
import test from 'node:test';
import { FULL_LANES, testTargetDir, workspaceTestTargets, workspaceMembers } from '../rust-test-lanes.mjs';
import { MODES } from '../ci-rust-tests.mjs';
import { archiveTargets, loadReleaseRust } from '../ci-rust-archives.mjs';

function runShell(script, directory, env) {
  const file = path.join(directory, 'selection.sh');
  fs.writeFileSync(file, script.replaceAll('\r', ''));
  return spawnSync(process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash', [file.replaceAll('\\', '/')], { encoding: 'utf8', env });
}

test('nextest shell selection and full CI use the catalogue lane definitions', () => {
  const { runners } = loadReleaseRust();
  assert.equal(MODES.full[0], FULL_LANES[0]);
  assert.equal(MODES.full[1], FULL_LANES[1]);
  for (const lane of FULL_LANES) {
    const args = execFileSync(process.execPath, ['scripts/rust-test-lanes.mjs', lane.lane], { encoding: 'utf8' }).trim().split('\n');
    assert.deepEqual(args, lane.targets);
    assert.deepEqual(args, lane.lane === 'workspace' ? workspaceTestTargets() : archiveTargets(lane.lane, runners));
  }
  const packages = new Set(FULL_LANES[0].targets.flatMap((arg, index, args) => args[index - 1] === '-p' ? [arg] : []));
  for (const name of workspaceMembers()) {
    assert.ok(packages.has(name), name);
  }
  assert.equal(FULL_LANES[0].targets.at(-1), '--tests');
});

test('local and CI target policy shares full targets and isolates feature lanes', () => {
  for (const env of [{}, { CARGO_TARGET_DIR: path.resolve('custom-target') }, { CI_TARGET_ROOT: path.resolve('ci-targets') }]) {
    for (const lane of FULL_LANES) {
      const fromShell = execFileSync(process.execPath, ['scripts/rust-test-lanes.mjs', lane.lane, 'target'], {
        encoding: 'utf8', env: { ...process.env, CI_TARGET_ROOT: '', CARGO_TARGET_DIR: '', ...env },
      });
      const normal = testTargetDir(lane, { env });
      assert.equal(fromShell, normal);
      assert.notEqual(testTargetDir(lane, { env, mode: 'embeddings' }), normal);
      assert.notEqual(testTargetDir(lane, { env, mode: 'platform' }), normal);
    }
    assert.equal(testTargetDir(FULL_LANES[1], { env }), path.join(testTargetDir(FULL_LANES[0], { env }), 'cli'));
  }
});

test('workspace membership refreshes on manifest changes and reuses unchanged metadata', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'workspace-members-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  fs.writeFileSync(path.join(temp, 'Cargo.toml'), 'first manifest');
  let calls = 0;
  const run = (binary) => {
    if (binary === 'git') return 'Cargo.toml\0';
    calls++;
    return JSON.stringify({ workspace_members: ['member'], packages: [{ id: 'member', name: calls === 1 ? 'original' : 'added' }, { id: 'dependency', name: 'unselected' }] });
  };
  assert.deepEqual(workspaceMembers(temp, run), ['original']);
  assert.deepEqual(workspaceMembers(temp, run), ['original']);
  assert.equal(calls, 1);
  fs.writeFileSync(path.join(temp, 'Cargo.toml'), 'new member manifest');
  assert.deepEqual(workspaceMembers(temp, run), ['added']);
  assert.equal(calls, 2);
});


test('nextest workspace warms the CLI first and uses the identical workspace selection', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'nextest-selection-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const log = path.join(temp, 'cargo-args');
  const source = fs.readFileSync('build.sh', 'utf8').match(/run_nextest_scope\(\) \{[\s\S]*?\n\}/)[0];
  const script = `${source}
prepare_dev_frontend_dist() { :; }
prepare_bundle_stubs() { :; }
cargo() {
  printf '%s\\n' "$*" >> "$SELECTION_LOG"
  case "$*" in *cli/Cargo.toml*) return 0 ;; *) return 42 ;; esac
}
run_nextest_scope workspace
`;
  const result = runShell(script, temp, { ...process.env, CARGO_TARGET_DIR: '', CI_TARGET_ROOT: '', SELECTION_LOG: log });
  assert.equal(result.status, 42, result.stderr);
  const lines = fs.readFileSync(log, 'utf8').trim().split('\n');
  assert.equal(lines.length, 2);
  assert.match(lines[0], /^test .*--locked --no-run --message-format=json$/);
  for (const [index, lane] of [FULL_LANES[1], FULL_LANES[0]].entries()) {
    assert.ok(lines[index].includes(`--manifest-path ${lane.manifest} ${lane.targets.join(' ')}`));
  }
});


test('focused CLI nextest uses the same lane and target as full CI', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'nextest-cli-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const log = path.join(temp, 'cargo-args');
  const source = fs.readFileSync('build.sh', 'utf8').match(/run_nextest_scope\(\) \{[\s\S]*?\n\}/)[0];
  const script = `${source}
cargo() { printf '%s\\n%s\\n' "$CARGO_TARGET_DIR" "$*" > "$SELECTION_LOG"; return 42; }
run_nextest_scope cli 'test(~opaque)'
`;
  const env = { ...process.env, CARGO_TARGET_DIR: '', CI_TARGET_ROOT: '', SELECTION_LOG: log };
  const result = runShell(script, temp, env);
  assert.equal(result.status, 42, result.stderr);
  const [target, args] = fs.readFileSync(log, 'utf8').trim().split('\n');
  assert.equal(target, testTargetDir(FULL_LANES[1], { env }));
  assert.ok(args.includes(`--manifest-path cli/Cargo.toml ${FULL_LANES[1].targets.join(' ')}`));
});

test('test-one qualifies the requested package, including aliases, before workspace execution', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'focused-package-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const source = fs.readFileSync('build.sh', 'utf8');
  const helper = source.match(/^rust_test_package\(\) \{[\s\S]*?^\}/m)[0];
  const start = source.indexOf('\n  test-one)\n') + '\n  test-one)\n'.length;
  const first = source.indexOf('    case "$test_scope" in', start);
  const dispatch = source.slice(start, source.indexOf('    case "$test_scope" in', first + 1));
  const log = path.join(temp, 'selection');
  const script = `${helper}
run_nextest_scope() { printf '%s\\n%s\\n' "$1" "$2" >> "$SELECTION_LOG"; }
dispatch_one() (
${dispatch}
)
for scope in codeterm-orchestration codeterm-client-shell app root shell plan cli; do dispatch_one test-one "$scope" dispatch; done
dispatch_one test-one unknown-scope dispatch && exit 99
exit 0
`;
  const result = runShell(script, temp, { ...process.env, SELECTION_LOG: log });
  assert.equal(result.status, 0, result.stderr);
  const rows = fs.readFileSync(log, 'utf8').trim().split('\n');
  const packages = ['codeterm-orchestration', 'codeterm-client-shell', 'codeterm-service-core', 'codeterm', 'codeterm', 'codeterm-plan', 'codeterm-cli'];
  assert.deepEqual(rows.filter((_, index) => index % 2), packages.map(pkg => `package(=${pkg}) & test(~dispatch)`));
  assert.deepEqual(rows.filter((_, index) => index % 2 === 0), packages.map(pkg => pkg === 'codeterm-cli' ? 'cli' : 'workspace'));
});

for (const fails of [false, true]) test(`CLI target selection restores caller state; compilation fails=${fails}`, t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'scope-target-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const source = fs.readFileSync('build.sh', 'utf8').match(/^run_nextest_scope\(\) \{[\s\S]*?^\}/m)[0];
  const log = path.join(temp, 'targets');
  const script = `${source}
SLOW_UNIT_TESTS_MEASUREMENT=off
cargo() { if [ "$1 $2" = 'nextest list' ]; then printf '%s\\n' "$CARGO_TARGET_DIR" >> "$SELECTION_LOG"; fi; printf '{}\\n'; return ${fails ? 42 : 0}; }
scripts/test-capped.sh() { printf 'Summary [0.001s] 1 tests run: 1 passed\\n'; }
export CARGO_TARGET_DIR="$FIXTURE_TARGET"
run_nextest_scope cli || [ "$?" = 42 ]
[ "$CARGO_TARGET_DIR" = "$FIXTURE_TARGET" ] || exit 91
run_nextest_scope util || [ "$?" = 42 ]
unset CARGO_TARGET_DIR
run_nextest_scope cli || [ "$?" = 42 ]
[ -z "\${CARGO_TARGET_DIR+x}" ] || exit 92
`;
  const original = path.join(temp, 'caller-target');
  const result = runShell(script, temp, { ...process.env, FIXTURE_TARGET: original, SELECTION_LOG: log });
  assert.equal(result.status, 0, result.stderr);
  const rows = fs.readFileSync(log, 'utf8').trim().split('\n').map(value => path.normalize(value));
  assert.deepEqual(rows.slice(0, 2), [path.join(original, 'cli'), original]);
});

test('reverse-dependency scopes prepare resources once before Cargo in a fresh checkout', t => {
  const temp = fs.mkdtempSync(path.join(os.tmpdir(), 'rdeps-resources-'));
  t.after(() => fs.rmSync(temp, { recursive: true, force: true }));
  const source = fs.readFileSync('build.sh', 'utf8').match(/^run_nextest_scope\(\) \{[\s\S]*?^\}/m)[0];
  const log = path.join(temp, 'events');
  const result = runShell(`${source}
prepare_dev_frontend_dist() { printf 'frontend\\n' >> "$SELECTION_LOG"; }
prepare_bundle_stubs() { printf 'bundle\\n' >> "$SELECTION_LOG"; }
cargo() { printf 'cargo %s\\n' "$*" >> "$SELECTION_LOG"; return 42; }
run_nextest_scope plugin 'rdeps(codeterm-plugin)' || [ "$?" = 42 ]
`, temp, { ...process.env, SELECTION_LOG: log });
  assert.equal(result.status, 0, result.stderr);
  const rows = fs.readFileSync(log, 'utf8').trim().split('\n');
  assert.deepEqual(rows.slice(0, 2), ['frontend', 'bundle']);
  assert.equal(rows.length, 3);
  assert.match(rows[2], /cargo nextest list .*--workspace/);
});
