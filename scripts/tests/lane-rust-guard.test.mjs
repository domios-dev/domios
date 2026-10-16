import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const guard = readFileSync(new URL('../lane-rust-guard.sh', import.meta.url), 'utf8');
const refusal = 'Rust tests and check-queue run in canonical through the Orchestrator; hand over the selector';
for (const command of ['test-one', 'nextest', 'check-queue', 'lane-check']) {
 for (const canonical of [false, true]) {
  for (const override of ['0', '1', 'true']) {
   test(`${command}: canonical=${canonical}, override=${override}`, () => {
    const result = spawnSync(bash, ['-c', `${guard}
git() {
 case "$*" in
  'rev-parse --show-toplevel') printf '%s\\n' '${canonical ? '/canonical checkout' : '/lane checkout'}' ;;
  'worktree list --porcelain') printf '%s\\n' 'worktree /canonical checkout' '' 'worktree /lane checkout' ;;
  *) return 99 ;;
 esac
}
CODETERM_ALLOW_LANE_RUST_TESTS=${override}
require_canonical_rust_tests ${command}
`], { encoding: 'utf8' });
    const blocked = command !== 'lane-check' && !canonical && override !== '1';
    assert.equal(result.status, blocked ? 2 : 0, result.stderr);
    assert.equal(result.stderr.trim(), blocked ? refusal : '');
    assert.equal(result.stdout, '');
   });
  }
 }
}
test('build checks the guard before build setup and peer dispatch', () => {
 const source = readFileSync(new URL('../../build.sh', import.meta.url), 'utf8');
 const call = source.indexOf('require_canonical_rust_tests "$CMD" || exit $?');
 assert.ok(call > 0 && call < source.indexOf('case "$CMD" in'));
 assert.ok(source.indexOf('source scripts/lane-rust-guard.sh') < call);
});
