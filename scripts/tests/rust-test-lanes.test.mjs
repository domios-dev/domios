import assert from 'node:assert/strict';
import path from 'node:path';
import fs from 'node:fs';
import os from 'node:os';
import { execFileSync, spawnSync } from 'node:child_process';
import test from 'node:test';
import { FULL_LANES, testTargetDir } from '../rust-test-lanes.mjs';
import { MODES } from '../ci-rust-tests.mjs';
import { archiveTargets, loadReleaseRust } from '../ci-rust-archives.mjs';

function runShell(script, directory, env) {
  const file = path.join(directory, 'selection.sh');
  fs.writeFileSync(file, script.replaceAll('\r', ''));
  return spawnSync('bash', [file.replaceAll('\\', '/')], { encoding: 'utf8', env });
}

test('nextest shell selection and full CI use the catalogue lane definitions', () => {
  const { runners } = loadReleaseRust();
  assert.equal(MODES.full[0], FULL_LANES[0]);
  assert.equal(MODES.full[1], FULL_LANES[1]);
  for (const lane of FULL_LANES) {
    const args = execFileSync(process.execPath, ['scripts/rust-test-lanes.mjs', lane.lane], { encoding: 'utf8' }).trim().split('\n');
    assert.deepEqual(args, lane.targets);
    assert.deepEqual(args, [...(lane.lane === 'workspace' ? ['--no-default-features'] : []), ...archiveTargets(lane.lane, runners)]);
  }
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
