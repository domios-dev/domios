// Caller: skill testing
import assert from 'node:assert/strict';
import { confirmCandidates } from './confirm-slow-unit-tests.mjs';
import test, { describe, before, after } from 'node:test';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, writeFileSync, readFileSync, copyFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { splitTestName, testKey } from './rust-test-inventory.mjs';
import { checkSlowTests, parseTimings, unitKey, validateBaseline, selectCandidates, candidateFilter, timingWarnings, regressionLimit, validateBenchmarks, checkBenchmarks } from './check-slow-unit-tests.mjs';

const entry = (name, seconds) => ({ binary: 'codeterm', test: name, measuredSeconds: seconds });
const slow = (key, seconds) => ({ key, measuredSeconds: seconds });
const baseline = (...entries) => ({ version: 1, entries });

test('parses nextest timings and keeps the slowest retry', () => {
  assert.deepEqual(parseTimings('\x1b[32m PASS [ 1.200s] codeterm a\x1b[0m\n FAIL [ 0.200s] codeterm a\n PASS [ 0.010s] ( 2/2) codeterm b\n Summary [ 1.210s] 2 tests run: 2 passed\n'), [entry('a', 1.2), entry('b', 0.01)]);
});

test('rejects incomplete and empty reports', () => {
  assert.throws(() => parseTimings('PASS [ 1.200s] codeterm a'), /summary/);
  assert.throws(() => parseTimings('Summary [ 0.000s] 0 tests run'), /no per-test/);
  assert.throws(() => parseTimings('PASS [ 0.200s] codeterm a\nSummary [ 1.000s] 2 tests run'), /incomplete/);
});

test('new slow unit fails above noise band and one second is permitted', () => {
  assert.deepEqual(checkSlowTests([entry('a', 1)], baseline()), []);
  assert.match(checkSlowTests([entry('a', 1.251)], baseline(), baseline(), [entry('a', 1.251)])[0], /new slow unit/);
});

test('existing slow units remain visible and can become fast', () => {
  assert.deepEqual(checkSlowTests([entry('a', 2)], baseline(slow('a', 1.5)), baseline(slow('a', 1.5)), [entry('a', 1.2)]), []);
  assert.deepEqual(checkSlowTests([entry('a', 0.2)], baseline(), baseline(slow('a', 1.5))), []);
});

test('baseline cannot gain tests or increase recorded durations', () => {
  assert.match(checkSlowTests([], baseline(slow('a', 2)), baseline())[0], /added baseline/);
  assert.match(checkSlowTests([], baseline(slow('a', 2)), baseline(slow('a', 1.5)))[0], /increased baseline/);
  assert.deepEqual(checkSlowTests([], baseline(slow('a', 1.1)), baseline(slow('a', 1.5))), []);
});

test('baseline identities are exact and unique', () => {
  assert.throws(() => validateBaseline(baseline(slow('a', 1.5), slow('a', 1.5))), /duplicate/);
  assert.throws(() => validateBaseline(baseline(slow('a', 0.5))), /exact/);
  assert.throws(() => validateBaseline({ version: 1, entries: [], wildcard: true }), /only/);
});

test('captured nextest output includes failing tests and timeout lower bounds', () => {
  const output = 'FAIL [ 0.210s] ( 1/2) codeterm a\nTIMEOUT [> 1.234s] (2/2) codeterm b\nSummary [ 1.234s] 2 tests run: 2 failed';
  assert.deepEqual(parseTimings(output), [
    {binary: 'codeterm', test: 'a', measuredSeconds: 0.21},
    {binary: 'codeterm', test: 'b', measuredSeconds: 1.234},
  ]);
});

describe('build nextest ratchet', () => {
  let root;
  before(() => {
    root = mkdtempSync(join(tmpdir(), 'slow-unit-gate-'));
    mkdirSync(join(root, 'scripts'));
    for (const file of ['check-slow-unit-tests.mjs', 'confirm-slow-unit-tests.mjs', 'build-gateway-output.mjs', 'rust-test-inventory.mjs']) copyFileSync(resolve('scripts', file), join(root, 'scripts', file));
    writeFileSync(join(root, 'scripts/slow-unit-tests.json'), JSON.stringify(baseline()));
    execFileSync('git', ['init', '-q', root]);
    execFileSync('git', ['-C', root, 'add', '.']);
    execFileSync('git', ['-C', root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'baseline']);
    const build = readFileSync('build.sh', 'utf8');
    const configuration = build.match(/^configure_app_test_cargo\(\) \{[\s\S]*?^\}/m)[0];
    const fn = build.match(/run_nextest_scope\(\) \{[\s\S]*?\n\}/)[0];
    writeFileSync(join(root, 'scripts/test-capped.sh'), '#!/usr/bin/env bash\ncat "$FIXTURE_REPORT"\nexit "$FIXTURE_RUN_EXIT"\n', {mode: 0o755});
    writeFileSync(join(root, 'gate.sh'), 'set -e\nenter_build_gateway() { :; }\nprepare_dev_frontend_dist() { :; }\nprepare_bundle_stubs() { :; }\nprepare_exceptional_worktree_target() { :; }\ncargo() { printf \"%s\\n\" \"$*\" >> \"$FIXTURE_CARGO_LOG\"; printf \"{}\\n\"; }\nCMD=nextest\n' + 'configure_dev_cargo() { :; }\nfind_app_test_archive() { return 1; }\n' + configuration + '\n' + fn + '\nrun_nextest_scope "${FIXTURE_SCOPE:-app}"\n');
  });
  after(() => rmSync(root, {recursive: true, force: true}));
  const run = (report, code, scope = 'app') => {
    writeFileSync(join(root, 'nextest.log'), report);
    writeFileSync(join(root, 'cargo-args.log'), '');
    return spawnSync('bash', ['gate.sh'], {cwd: root, encoding: 'utf8', env: {
      ...process.env, FIXTURE_SCOPE: scope, FIXTURE_CARGO_LOG: join(root, 'cargo-args.log'), FIXTURE_REPORT: join(root, 'nextest.log'), FIXTURE_RUN_EXIT: String(code), SLOW_UNIT_TESTS_BASE: 'HEAD',
    }});
  };
  const output = (status, seconds) => `${status} [ ${seconds}s] (1/1) codeterm opaque\nSummary [ ${seconds}s] 1 tests run\n`;
  test('accepts a fast executed unit', () => assert.equal(run(output('PASS', 0.2), 0).status, 0));
  test('recording scope compiles both isolated libraries once without app preparation', () => {
    assert.equal(run(output('PASS', 0.2), 0, 'input-platform').status, 0);
    const calls = readFileSync(join(root, 'cargo-args.log'), 'utf8').trim().split('\n');
    const compile = calls.filter((call) => call.startsWith('nextest list '));
    assert.equal(compile.length, 1);
    assert.match(compile[0], /--lib -p codeterm-input-macos -p codeterm-input-windows/);
    assert.doesNotMatch(compile[0], /--workspace|-p codeterm(?: |$)/);
  });
  test('rejects a new slow unit', () => {
    const slow = run(output('PASS', 1.251), 0);
    assert.equal(slow.status, 1);
    assert.match(slow.stderr, /new slow unit/);
  });
  test('preserves a failed test exit', () => assert.equal(run(output('FAIL', 0.2), 100).status, 100));
  test('preserves a compilation failure exit', () => assert.equal(run('compiler failed\n', 100).status, 100));
  test('rejects an incomplete successful runner log', () => assert.equal(run('PASS [ 0.2s] codeterm opaque\n', 0).status, 1));
  test('serial confirmation reuses exact binaries and retains test failure status', () => {
    const calls = [];
    const code = confirmCandidates([{binary: 'codeterm', test: 'a'}, {binary: 'codeterm', test: 'b'}], {
      manifest: 'source/Cargo.toml', binaries: 'same-binaries.json', cargo: 'same-cargo.json',
      output: join(root, 'alone.log'), print: () => {},
    }, (command, args, options) => {
      calls.push({command, args, options});
      const name = calls.length === 1 ? 'a' : 'b';
      return {status: name === 'a' ? 0 : 100, stdout: `PASS [ 0.2s] codeterm ${name}\nSummary [ 0.2s] 1 tests run\n`, stderr: ''};
    });
    assert.equal(code, 100);
    assert.equal(calls.length, 2);
    for (const call of calls) {
      assert.equal(call.command, 'bash');
      assert.equal(call.args[call.args.indexOf('--binaries-metadata') + 1], 'same-binaries.json');
      assert.equal(call.args[call.args.indexOf('--cargo-metadata') + 1], 'same-cargo.json');
      assert.equal(call.args[call.args.indexOf('--test-threads') + 1], '1');
      assert.equal(call.options.env.CARGO_TEST_BUILD_CMD, '');
      for (const flag of ['--no-run', '--manifest-path', '--lib', '-p', '--no-default-features']) assert.ok(!call.args.includes(flag));
    }
    assert.equal(parseTimings(readFileSync(join(root, 'alone.log'), 'utf8')).length, 2);
  });
  test('first crate baseline seeds independently then forbids additions after landing', () => {
    const path = join(root, 'scripts/slow-crate-unit-tests.json');
    writeFileSync(path, JSON.stringify(baseline(slow('existing', 2))));
    assert.equal(run(output('PASS', 0.2), 0).status, 0);
    execFileSync('git', ['-C', root, 'add', 'scripts/slow-crate-unit-tests.json']);
    execFileSync('git', ['-C', root, '-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-qm', 'crate baseline']);
    writeFileSync(path, JSON.stringify(baseline(slow('existing', 2), slow('new', 2))));
    assert.match(run(output('PASS', 0.2), 0).stderr, /added baseline/);
    writeFileSync(path, JSON.stringify(baseline(slow('existing', 2.1))));
    assert.match(run(output('PASS', 0.2), 0).stderr, /increased baseline/);
    writeFileSync(path, JSON.stringify(baseline()));
    assert.equal(run(output('PASS', 0.2), 0).status, 0);
  });

  test('serial confirmation refuses duplicates before any execution', () => {
    assert.throws(() => confirmCandidates([entry('a', 2), entry('a', 2)], {output: join(root, 'duplicate.log')},
      () => { throw new Error('must not execute'); }), /duplicate serial candidate/);
  });

  test('serial confirmation reuses the selected archive without compiling or switching binaries', () => {
    let args;
    assert.equal(confirmCandidates([{binary: 'codeterm', test: 'opaque'}], {
      archive: 'opaque-archive.tar.zst', output: join(root, 'archive-alone.log'), print: () => {},
    }, (_command, selected, options) => {
      args = selected;
      assert.equal(options.env.CARGO_TEST_BUILD_CMD, '');
      return { status: 0, stdout: 'PASS [ 0.2s] codeterm opaque\nSummary [ 0.2s] 1 tests run\n' };
    }), 0);
    assert.equal(args[args.indexOf('--archive-file') + 1], 'opaque-archive.tar.zst');
    assert.ok(args.includes('--workspace-remap'));
    assert.ok(!args.includes('--binaries-metadata'));
  });

});

test('parallel noise is confirmed alone before refusal', () => {
  assert.deepEqual(checkSlowTests([entry('a', 2)], baseline(), baseline(), [entry('a', 0.2)]), []);
  assert.match(checkSlowTests([entry('a', 2)], baseline())[0], /missing alone timing/);
});

test('only new or over-baseline slow candidates need confirmation', () => {
  assert.deepEqual(selectCandidates([entry('a', 1.2), entry('b', 1), entry('c', 1.1)], baseline(slow('a', 1.5))), [entry('c', 1.1)]);
  assert.match(checkSlowTests([entry('a', 2)], baseline(slow('a', 1.5)), baseline(slow('a', 1.5)), [entry('a', 1.801)])[0], /regression/);
});

test('candidate filters contain exact data and reject syntax injection', () => {
  assert.equal(candidateFilter([entry('a::b', 2)]), '(binary_id(=codeterm) & test(=a::b))');
  assert.throws(() => candidateFilter([entry('a) | all()', 2)]), /unsafe/);
  assert.throws(() => parseTimings('PASS [ 1.2s] codeterm a)\nSummary [ 1.2s] 1 tests run'), /invalid/);
});

test('serial reports can concatenate separate one-test executions', () => {
  assert.deepEqual(parseTimings('PASS [ 0.2s] codeterm a\nSummary [ 0.2s] 1 tests run\nFAIL [ 1.2s] codeterm b\nSummary [ 1.2s] 1 tests run'), [entry('a', 0.2), entry('b', 1.2)]);
});

test('new alone timings in the 1s to 1.25s band warn without failing', () => {
  for (const seconds of [1.04, 1.25]) {
    assert.deepEqual(checkSlowTests([entry('a', 2)], baseline(), baseline(), [entry('a', seconds)]), []);
    assert.match(timingWarnings([entry('a', seconds)], baseline())[0], /1s unit target/);
  }
  assert.deepEqual(timingWarnings([entry('a', 1)], baseline()), []);
});

test('regression noise band uses the larger of twenty percent and quarter second', () => {
  for (const [recorded, measured] of [[1.191, 1.208], [1.05, 1.052], [1.1, 1.35], [2, 2.4]]) {
    const document = baseline(slow('a', recorded));
    assert.deepEqual(checkSlowTests([entry('a', 3)], document, document, [entry('a', measured)]), []);
    assert.match(timingWarnings([entry('a', measured)], document)[0], /WARNING/);
    assert.match(checkSlowTests([entry('a', 3)], document, document, [entry('a', regressionLimit(recorded) + 0.001)])[0], /regression/);
  }
});

test('benchmark exemptions require exact unique identities and an explicit reason', () => {
  const exemption = {binary: 'codeterm', test: 'throughput', reason: 'Measures throughput by repeated replay'};
  assert.deepEqual(validateBenchmarks(baseline(exemption)), [exemption]);
  assert.throws(() => validateBenchmarks(baseline({...exemption, test: '*'})), /exact/);
  assert.throws(() => validateBenchmarks(baseline({...exemption, reason: ''})), /reason/);
  assert.throws(() => validateBenchmarks(baseline(exemption, exemption)), /unique/);
  assert.match(checkBenchmarks(baseline(exemption), baseline())[0], /added/);
  assert.deepEqual(checkBenchmarks(baseline(), baseline(exemption)), []);
});

test('an audited benchmark is exempt from timing only and cannot become a baseline allowance', () => {
  const exemptions = [{binary: 'codeterm', test: 'throughput', reason: 'Measures throughput'}];
  assert.deepEqual(selectCandidates([entry('throughput', 21), entry('unit', 2)], baseline(), exemptions), [entry('unit', 2)]);
  assert.deepEqual(checkSlowTests([entry('throughput', 21)], baseline(), baseline(), [], exemptions), []);
  assert.match(checkSlowTests([], baseline(slow('throughput', 21)), baseline(slow('throughput', 21)), [], exemptions)[0], /must not be baselined/);
});

test('a slow test keeps its baseline across a crate move, whatever the crate is named', () => {
  const key = 'storage::probe::native_tests::slow_probe';
  const document = baseline(slow(key, 8.5));
  const moved = { binary: 'codeterm-persist-engine', test: 'probe::native_tests::slow_probe', measuredSeconds: 8.6 };
  assert.equal(unitKey(key), key);
  assert.equal(unitKey(moved.test), testKey(splitTestName(moved.test)));
  assert.deepEqual(checkSlowTests([moved], document, document, [moved]), []);
  assert.match(checkSlowTests([moved], document, document, [{ ...moved, measuredSeconds: 12 }])[0], /slow unit regression: codeterm-persist-engine probe::native_tests::slow_probe \(key storage::probe/);
  const back = { binary: 'codeterm', test: key, measuredSeconds: 2 };
  assert.deepEqual(checkSlowTests([back], baseline(slow('probe::native_tests::slow_probe', 8.5)), baseline(slow('probe::native_tests::slow_probe', 8.5)), [back]), []);
  const renamed = { ...moved, test: 'probe::tests::slow_probe' };
  assert.match(checkSlowTests([renamed], document, document, [renamed])[0], /new slow unit/);
});

test('an ambiguous facade match gives no allowance', () => {
  const document = baseline(slow('alpha::tests::slow', 3), slow('beta::tests::slow', 3));
  const moved = { binary: 'codeterm-alpha', test: 'tests::slow', measuredSeconds: 3 };
  assert.match(checkSlowTests([moved], document, document, [moved])[0], /new slow unit/);
});

test('a base-ref baseline still keyed by binary and test is read through the same key', () => {
  const legacy = baseline({ binary: 'codeterm', test: 'sentinel::scheduler_tests::slow', measuredSeconds: 4.1 }, { binary: 'codeterm-plan', test: 'tests::slow', measuredSeconds: 1.3 });
  assert.deepEqual(checkSlowTests([], baseline(slow('sentinel::scheduler_tests::slow', 4.1), slow('tests::slow', 1.3)), legacy), []);
  assert.throws(() => validateBaseline(legacy), /exact module_path::test_name key/);
});
