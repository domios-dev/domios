// Caller: build.sh script-guards
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
test('one atomic root and core archive per SHA is reused by app selectors without compiling', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'app-archive-'));
  try {
    const result = spawnSync(bash, ['-c', `
      set -euo pipefail
      source "$1/scripts/nextest-archive.sh"
      head=opaque-sha
      CT_BUILD_JOB_ID=opaque-job
      git() { case "$1" in rev-parse) echo "$head" ;; diff) return 0 ;; esac; }
      ct_gateway_dir() { echo "$PWD/gateway"; }
      enter_cargo_gateway() { echo GATEWAY >> calls.log; }
      check_queue_worktree() { echo "$PWD"; }
      enter_stable_build_path() { :; }
      configure_dev_cargo() { :; }
      prepare_dev_frontend_dist() { :; }
      prepare_bundle_stubs() { :; }
      cargo() {
        printf '%s\\n' "$*" >> calls.log
        while [ "$1" != --archive-file ]; do shift; done
        printf artifact > "$2"
      }
      scripts/test-capped.sh() {
        [ "$CARGO_TEST_CMD" = 'cargo nextest run' ] && [ -z "$CARGO_TEST_BUILD_CMD" ]
        printf 'RUN %s\\n' "$*" >> calls.log
      }
      build_app_test_archive "$head"
      build_app_test_archive "$head" --measure opaque-a opaque-b
      find_app_test_archive
      run_archived_app_test opaque-selector
      head=other-sha
      if find_app_test_archive; then exit 91; fi
      cargo() { while [ "$1" != --archive-file ]; do shift; done; printf partial > "$2"; return 43; }
      if build_app_test_archive "$head"; then exit 92; else [ "$?" = 43 ]; fi
      [ ! -e "$(app_test_archive_path "$head")" ]
    `, 'fixture', root.replaceAll('\\', '/')], { cwd: directory, encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const calls = fs.readFileSync(path.join(directory, 'calls.log'), 'utf8').trim().split('\n');
    const builds = calls.filter(line => line.startsWith('nextest archive'));
    assert.equal(builds.length, 1);
    assert.match(builds[0], /-p codeterm-service-core -p codeterm --lib --cargo-profile test/);
    assert.match(builds[0], /opaque-job\.tar\.zst/);
    assert.equal(fs.readFileSync(path.join(directory, 'gateway/test-archives/opaque-sha/app.tar.zst'), 'utf8'), 'artifact');
    assert.ok(calls.some(line => /^RUN --archive-file .*app\.tar\.zst --workspace-remap .* -p codeterm-service-core --lib --no-tests=fail opaque-selector$/.test(line)));
    assert.match(result.stdout, /selector=opaque-a exit=0 wall=\d+s/);
    assert.match(result.stdout, /selector=opaque-b exit=0 wall=\d+s/);
    assert.match(result.stdout, /selector=opaque-selector exit=0 wall=\d+s/);
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
});

test('both app runners use archive reuse and serial slow confirmations retain the same archive', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const nextest = source.match(/^run_nextest_scope\(\) \{[\s\S]*?^\}/m)[0];
  const focused = source.slice(source.indexOf('  test-one)')).match(/^      app\)[\s\S]*?^        ;;/m)[0];
  assert.match(nextest, /find_app_test_archive/);
  assert.match(nextest, /reuse_args=\(--archive-file/);
  assert.match(nextest, /confirmation_args=\(--archive-file/);
  assert.match(focused, /find_app_test_archive[\s\S]*run_archived_app_test/);
});
