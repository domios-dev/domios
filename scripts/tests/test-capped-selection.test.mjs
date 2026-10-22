import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';

const root = path.resolve(import.meta.dirname, '../..');
const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const posix = value => value.replaceAll('\\', '/');

const libtestZero = 'running 0 tests\n\ntest result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 42 filtered out\n';
const libtestIgnored = 'running 1 test\ntest opaque::case ... ignored\n\ntest result: ok. 0 passed; 0 failed; 1 ignored; 0 measured; 41 filtered out\n';
const libtestPass = 'running 1 test\ntest opaque::case ... ok\n\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 41 filtered out\n';
const nextestZero = '    Starting 0 tests across 1 binary (42 tests skipped)\n     Summary [   0.010s] 0 tests run: 0 passed, 42 skipped\n';
const nextestPass = '    Starting 2 tests across 1 binary (40 tests skipped)\n     Summary [   0.210s] 2 tests run: 2 passed (1 slow), 40 skipped\n';

function runWrapper(args, { output = '', exit = 0, env = {} } = {}) {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'capped-selection-'));
  try {
    fs.writeFileSync(path.join(directory, 'output.txt'), output);
    fs.writeFileSync(path.join(directory, 'runner.sh'), `cat "${posix(directory)}/output.txt"\nexit ${exit}\n`);
    const result = spawnSync(bash, [posix(path.join(root, 'scripts/test-capped.sh')), ...args], {
      cwd: directory,
      encoding: 'utf8',
      env: {
        ...process.env,
        HOME: posix(directory),
        CARGO_TEST_CMD: `bash ${posix(directory)}/runner.sh`,
        CARGO_TEST_BUILD_CMD: '',
        CODETERM_TEST_HOST_TRIPLE: 'x86_64-unknown-linux-gnu',
        TEST_CAP_SECS: '60',
        CT_BUILD_ARTIFACT_DIR: '',
        ...env,
      },
    });
    return { status: result.status, text: result.stdout + result.stderr };
  } finally { fs.rmSync(directory, { recursive: true, force: true }); }
}

const crateArgs = ['--manifest-path', 'src-tauri/Cargo.toml', '-p', 'codeterm-opaque', '--lib'];
const archiveArgs = ['--archive-file', 'opaque.tar.zst', '--workspace-remap', 'src-tauri', '--filter-expr', 'package(=codeterm-opaque) & kind(lib)', '--no-tests=fail'];

test('a filtered libtest run that matches nothing exits 97 naming the filter and crate', () => {
  const result = runWrapper([...crateArgs, 'opaque_missing'], { output: libtestZero });
  assert.equal(result.status, 97, result.text);
  assert.match(result.text, /ZERO MATCHES: filter opaque_missing selected no tests in codeterm-opaque\./);
});

test('a filtered run whose every match is ignored is not a pass', () => {
  const result = runWrapper([...crateArgs, 'opaque::case'], { output: libtestIgnored });
  assert.equal(result.status, 97, result.text);
});

test('a filtered run that prints no test summary is not a pass', () => {
  const result = runWrapper([...crateArgs, 'opaque::case'], { output: 'Finished `test` profile\n' });
  assert.equal(result.status, 97, result.text);
});

test('an archived nextest run that selects zero tests exits 97 even when the runner exits 0', () => {
  const result = runWrapper([...archiveArgs, 'opaque_selector'], { output: nextestZero });
  assert.equal(result.status, 97, result.text);
  assert.match(result.text, /ZERO MATCHES: filter opaque_selector/);
});

test('a nextest filterset alone is a selection that must run a test', () => {
  const result = runWrapper(['--filter-expr', 'test(opaque)', '--test-threads', '1', '--no-tests=fail'], { output: nextestZero });
  assert.equal(result.status, 97, result.text);
  assert.match(result.text, /ZERO MATCHES: filter test\(opaque\)/);
});

test('the gateway artifact mode applies the same rule to its captured log', () => {
  const artifacts = fs.mkdtempSync(path.join(os.tmpdir(), 'capped-artifacts-'));
  try {
    const result = runWrapper([...crateArgs, 'opaque_missing'], { output: libtestZero, env: { CT_BUILD_ARTIFACT_DIR: posix(artifacts) } });
    assert.equal(result.status, 97, result.text);
  } finally { fs.rmSync(artifacts, { recursive: true, force: true }); }
});

test('filtered runs that pass a test stay green for libtest and nextest', () => {
  assert.equal(runWrapper([...crateArgs, 'opaque::case'], { output: libtestPass }).status, 0);
  assert.equal(runWrapper([...archiveArgs, 'opaque_selector'], { output: nextestPass }).status, 0);
});

test('an explicit no-tests allowance and an unfiltered run are never failed for running nothing', () => {
  assert.equal(runWrapper(['--filter-expr', 'test(opaque)', '--no-tests=warn'], { output: nextestZero }).status, 0);
  assert.equal(runWrapper(crateArgs, { output: libtestZero }).status, 0);
});

test('a failing runner keeps its own exit code', () => {
  assert.equal(runWrapper([...crateArgs, 'opaque::case'], { output: libtestZero, exit: 101 }).status, 101);
});

test('every test-one scope sends its filter through the fail-closed runner', () => {
  const source = fs.readFileSync(path.join(root, 'build.sh'), 'utf8');
  const start = source.indexOf('\n  test-one)\n');
  const block = source.slice(start, source.indexOf('\n  nextest)\n', start));
  const dispatch = block.lastIndexOf('case "$test_scope" in');
  assert.match(block.slice(0, dispatch), /run_nextest_scope workspace "package\(=\$test_package\) & test\(~\$test_filter\)"/);
  const arms = block.slice(dispatch).split(/\n {6}(?=[a-z*][^\n]*\) )|\n {6}(?=[a-z*][^\n]*\)\n)/).slice(1);
  assert.ok(arms.length > 20, 'test-one scope arms were not found');
  for (const arm of arms) {
    const scope = arm.slice(0, arm.indexOf(')'));
    if (scope === '*') continue;
    const runs = arm.match(/scripts\/test-capped\.sh(?! --exec)[^\n]*"\$test_filter"|run_archived_app_test "\$test_filter"/g) ?? [];
    assert.ok(runs.length > 0, `test-one ${scope} does not route its filter through scripts/test-capped.sh`);
  }
});
