import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { parse } from 'yaml';
import { parseWarnings, deduplicate, makeBaseline, ratchet, assertBaselineShrinks, capture, renderReport } from '../collect-rust-warnings.mjs';
const fixture = name => fs.readFileSync(new URL('../../tests/fixtures/rust-warnings/' + name, import.meta.url), 'utf8');
const opts = { platform: 'Windows', target: 'x86_64-pc-windows-msvc' };
const warnings = () => parseWarnings(fixture('windows.txt'), opts);
test('Cargo JSON keeps warning location, lint, crate and ANSI-free snippet', () => {
  const [w] = parseWarnings(fixture('cargo.jsonl'), opts);
  assert.equal(w.crate, 'codeterm');
  assert.equal(w.file, 'src-tauri/src/service/local_recording/platform/windows/lifecycle.rs');
  assert.equal(w.line, 3);
  assert.equal(w.code, 'unused_imports');
  assert.equal(w.message, 'unused import: `Arc`');
  assert.ok(w.rendered.includes('-->'));
  assert.ok(!w.rendered.includes('\x1b'));
});
test('Windows text collects both imports, unused mut and both dead methods without summary duplicates', () => {
  const records = warnings();
  assert.equal(records.length, 5);
  assert.deepEqual(records.map(w => w.code).sort(), ['dead_code', 'dead_code', 'unused_imports', 'unused_imports', 'unused_mut']);
  assert.equal(records.find(w => w.code === 'unused_mut').line, 740);
  assert.equal(records.find(w => w.message.includes('request_stop')).line, 151);
  assert.equal(records.find(w => w.message.includes('feed_pty_output_inner')).line, 23);
  assert.ok(records.every(w => w.crate === 'codeterm'));
});
test('a failed build reports its compile error, never Cargo status lines, as warnings', () => {
  const text = '   Compiling fixture-crate v0.1.0\nerror[E0599]: no variant named `Other`\nwarning: build failed, waiting for other jobs to finish...\nerror: could not compile `fixture-crate`\n';
  assert.deepEqual(parseWarnings(text, { platform: 'Linux', target: 'x86_64-unknown-linux-gnu' }), []);
});
test('JSON and text warnings deduplicate by target/file/line/lint/message; other targets remain', () => {
  const records = parseWarnings(fixture('windows.txt') + fixture('cargo.jsonl'), opts);
  assert.equal(records.length, 5);
  assert.equal(deduplicate([...records, ...records]).length, 5);
  assert.equal(deduplicate([...records, { ...records[0], target: 'aarch64-pc-windows-msvc' }]).length, 6);
});
test('ratchet rejects a replacement at equal count and allows removal; baselines only shrink', () => {
  const before = warnings();
  const baseline = makeBaseline(before, [opts.target]);
  const after = before.slice(1);
  assert.equal(ratchet(after, baseline, [opts.target]).added.length, 0);
  assert.equal(ratchet(after, baseline, [opts.target]).removed.length, 1);
  assertBaselineShrinks(makeBaseline(after, [opts.target]), baseline);
  const replacement = [...after, { ...before[0], message: 'new warning' }];
  assert.equal(ratchet(replacement, baseline, [opts.target]).added.length, 1);
  assert.throws(() => assertBaselineShrinks(makeBaseline(replacement, [opts.target]), baseline), /only shrink/);
  assert.throws(() => assertBaselineShrinks({ initialized: false }, baseline), /Cannot reset/);
  const other = { ...before[0], target: 'aarch64-pc-windows-msvc' };
  const mixed = makeBaseline([...before, other], [opts.target, other.target]);
  assertBaselineShrinks(makeBaseline([...before, other], [other.target, opts.target]), mixed);
  assert.throws(() => assertBaselineShrinks({ ...mixed, counts: { [opts.target]: 0, [other.target]: 1 } }, mixed), /do not match/);
  assert.equal(ratchet(before, { initialized: false }, []).bootstrap, true);
});
test('capture saves stdout/stderr and retains failed command exit', async () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'rust-warnings-'));
  try {
    const log = path.join(dir, 'output.log');
    const code = await capture(process.execPath, ['-e', 'process.stdout.write("out");process.stderr.write("warning: fixture\\n");process.exitCode=7'], log);
    assert.equal(code, 7);
    assert.match(fs.readFileSync(log, 'utf8'), /warning: fixture/);
    assert.match(fs.readFileSync(log, 'utf8'), /out/);
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
test('report includes per-target/lint/file counts and rendered diagnostics', () => {
  const report = renderReport(warnings(), [opts.target], { bootstrap: false, added: [], removed: [] });
  assert.match(report, /x86_64-pc-windows-msvc \| 5/);
  assert.match(report, /unused_imports \| 2/);
  assert.match(report, /windows.rs \| 2/);
  assert.match(report, /let mut child/);
});
const workflow = name => parse(fs.readFileSync(new URL('../../.github/workflows/' + name + '.yml', import.meta.url), 'utf8'));
test('every active release compiler, Rust test and clippy job uploads through the shared action', () => {
  for (const name of ['release-macos-build', 'release-linux-arm64', 'release-linux', 'release-windows']) {
    const steps = workflow(name).jobs.build.steps;
    const artifact = steps.find(s => s.uses === './.github/actions/rust-warnings');
    assert.equal(artifact.if, 'always()');
    assert.equal(artifact['continue-on-error'], true);
    assert.ok(steps.some(s => s.run?.includes('collect-rust-warnings.mjs run')));
  }
  for (const id of ['rust-tests', 'rust-platform', 'clippy-linux']) {
    assert.ok(workflow('tests').jobs[id].steps.some(s => s.uses === './.github/actions/rust-warnings'));
  }
  assert.deepEqual(workflow('release').jobs['rust-warnings'].needs, ['prepare', 'quick-checks', 'tests', 'macos', 'linux-arm64', 'linux-x86_64', 'windows']);
  assert.equal(workflow('release').jobs['rust-warnings'].if, 'always()');
  assert.equal(workflow('tests').jobs['rust-warnings'].if, 'always()');
});

test('summary CLI persists a complete seed, preserves unbuilt targets, reports a new warning before failing', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'warning-report-'));
  try {
    fs.mkdirSync(path.join(dir, 'scripts'));
    fs.mkdirSync(path.join(dir, 'inputs'));
    const baselineFile = path.join(dir, 'scripts/rust-warnings-baseline.json');
    const inputFile = path.join(dir, 'inputs/rust-warnings-fixture.json');
    const baseline = makeBaseline(warnings(), [opts.target]);
    fs.writeFileSync(baselineFile, JSON.stringify({ version: 1, initialized: false, targets: [], counts: {}, warnings: [] }));
    fs.writeFileSync(inputFile, JSON.stringify({ ...opts, complete: true, warnings: warnings() }));
    const script = path.resolve('scripts/rust-warnings-report.mjs');
    const run = env => spawnSync(process.execPath, [script, 'inputs', 'report'], { cwd: dir, encoding: 'utf8', env: { ...process.env, WARNING_ARTIFACTS: '["rust-warnings-fixture"]', WARNING_SEED: 'true', ...env } });
    assert.equal(run().status, 0);
    assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'report/rust-warnings-baseline.json'))).counts[opts.target], 5);
    fs.writeFileSync(baselineFile, JSON.stringify(baseline));
    fs.writeFileSync(inputFile, JSON.stringify({ ...opts, complete: true, warnings: [{ ...warnings()[0], message: 'new lint' }] }));
    assert.equal(run().status, 1);
    const report = JSON.parse(fs.readFileSync(path.join(dir, 'report/rust-warnings-report.json')));
    assert.equal(report.decision.added.length, 1);
    assert.ok(!fs.existsSync(path.join(dir, 'report/rust-warnings-baseline.json')));
    assert.ok(fs.readFileSync(path.join(dir, 'report/rust-warnings-report.md'), 'utf8').includes('1 new'));
    fs.writeFileSync(inputFile, JSON.stringify({ platform: 'Linux', target: 'linux-fixture', complete: true, warnings: [] }));
    assert.equal(run().status, 0);
    assert.equal(JSON.parse(fs.readFileSync(path.join(dir, 'report/rust-warnings-baseline.json'))).counts[opts.target], 5);
    fs.writeFileSync(inputFile, JSON.stringify({ ...opts, complete: false, warnings: [] }));
    assert.equal(run().status, 0);
    assert.ok(!fs.existsSync(path.join(dir, 'report/rust-warnings-baseline.json')));
    fs.rmSync(path.join(dir, 'report'), { recursive: true });
    assert.equal(run({ WARNING_ARTIFACTS: '["missing"]' }).status, 0);
    assert.ok(!fs.existsSync(path.join(dir, 'report/rust-warnings-baseline.json')));
  } finally { fs.rmSync(dir, { recursive: true, force: true }); }
});
