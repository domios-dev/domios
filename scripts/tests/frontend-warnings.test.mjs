import assert from 'node:assert/strict';
import test from 'node:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { buildFrontend } from '../frontend-build.mjs';
import { parse as parseYaml } from 'yaml';
import { parseFrontendWarnings, frontendWarningCollection } from '../frontend-warnings.mjs';
import { makeBaseline, ratchet } from '../collect-rust-warnings.mjs';

const options = { platform: 'win32', root: 'C:/lane' };
const ineffective = '[INEFFECTIVE_DYNAMIC_IMPORT] Warning: C:\\lane\\src\\lib\\bridge.ts is dynamically imported by src/Memory.tsx but also statically imported by src/App.tsx, dynamic import will not move module into another chunk.';

test('Vite warning identities normalize roots and retain complete diagnostics', () => {
  const input = `\x1b[33m${ineffective}\x1b[0m\n\n[plugin builtin:vite-reporter]\n(!) Some chunks are larger than 500 kB after minification. Consider:\n- Adjust chunk size limit for this warning via build.chunkSizeWarningLimit.\n\n[PLUGIN_TIMINGS] Warning: Your build spent significant time in plugins. Here is a breakdown:\n  - fixture (50%)\n`;
  const warnings = parseFrontendWarnings(input, options);
  assert.equal(warnings.length, 3);
  const dynamic = warnings.find(w => w.code === 'INEFFECTIVE_DYNAMIC_IMPORT');
  assert.equal(dynamic.file, 'src/lib/bridge.ts');
  assert.equal(dynamic.target, 'frontend');
  assert.ok(!dynamic.rendered.includes('\x1b'));
  assert.ok(warnings.find(w => w.code === 'CHUNK_SIZE_LIMIT').rendered.includes('build.chunkSizeWarningLimit'));
  assert.ok(warnings.find(w => w.code === 'PLUGIN_TIMINGS').rendered.includes('fixture (50%)'));
  assert.deepEqual(parseFrontendWarnings(ineffective + '\n\n' + ineffective, options), [dynamic]);
});

test('frontend warnings use the existing identity ratchet without treating failures as complete', () => {
  const before = parseFrontendWarnings(ineffective, options);
  const baseline = makeBaseline(before, ['frontend']);
  assert.equal(frontendWarningCollection('', { ...options, complete: false }, baseline).complete, false);
  const removed = frontendWarningCollection('vite built in 1s\n', { ...options, complete: true }, baseline);
  assert.deepEqual(removed.warnings, []);
  assert.equal(removed.decision.removed.length, 1);
  const replacement = parseFrontendWarnings('[UNKNOWN_CODE] Warning: fixture diagnostic', options);
  assert.equal(ratchet(replacement, baseline, ['frontend']).added.length, 1);
  assert.deepEqual(parseFrontendWarnings('error TS0001: fixture\ndist/index.js 300 kB\n', options), []);
});

test('unrecognized Vite warnings are collected, and Rust identities remain independent', () => {
  const [warning] = parseFrontendWarnings('Warning: fixture\n  detail\n', options);
  assert.equal(warning.code, 'VITE_WARNING');
  assert.equal(warning.message, 'Warning: fixture');
  const rust = { ...warning, target: 'x86_64-pc-windows-msvc', crate: 'codeterm' };
  const baseline = makeBaseline([rust], [rust.target]);
  const result = ratchet([warning], baseline, ['frontend']);
  assert.deepEqual(result.added, [warning]);
  assert.deepEqual(result.removed, []);
});

test('frontend build records failed type checks and preserves their exit without bundling', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'frontend-warnings-'));
  try {
    fs.mkdirSync(path.join(root, 'scripts'));
    fs.writeFileSync(path.join(root, 'scripts/frontend-warnings-baseline.json'), JSON.stringify(makeBaseline([], [])));
    let calls = 0;
    const run = async (_program, _args, log) => { calls++; fs.appendFileSync(log, 'error TS0001: fixture\n'); return 7; };
    assert.equal(await buildFrontend({ root, platform: 'fixture', run }), 7);
    assert.equal(calls, 1);
    const document = JSON.parse(fs.readFileSync(path.join(root, 'ci-rust-warnings/rust-warnings-fixture-frontend.json')));
    assert.equal(document.complete, false);
    assert.deepEqual(document.warnings, []);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('a successful bundle with a new warning fails the shared ratchet and saves its artifact', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'frontend-warnings-'));
  try {
    fs.mkdirSync(path.join(root, 'scripts'));
    fs.writeFileSync(path.join(root, 'scripts/frontend-warnings-baseline.json'), JSON.stringify(makeBaseline([], [])));
    let calls = 0;
    const run = async (_program, _args, log) => { if (++calls === 2) fs.appendFileSync(log, 'Warning: fixture\n'); return 0; };
    assert.equal(await buildFrontend({ root, platform: 'win32', run }), 1);
    assert.equal(calls, 2);
    const workflow = parseYaml(fs.readFileSync(new URL('../../.github/workflows/tests.yml', import.meta.url), 'utf8'));
    const upload = workflow.jobs['frontend-windows'].steps.find(step => step.with?.name === 'rust-warnings-win32-frontend');
    assert.equal(upload.if, 'always()');
    const document = JSON.parse(fs.readFileSync(path.join(root, upload.with.path)));
    assert.equal(document.complete, true);
    assert.equal(document.decision.added.length, 1);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});

test('shared summary accepts the observed frontend warning and preserves independent Rust ratcheting', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'frontend-summary-'));
  try {
    fs.mkdirSync(path.join(root, 'scripts'));
    fs.mkdirSync(path.join(root, 'artifacts'));
    const baseline = JSON.parse(fs.readFileSync(new URL('../frontend-warnings-baseline.json', import.meta.url), 'utf8'));
    const frontend = parseFrontendWarnings(baseline.warnings.map(warning => warning.code === 'CHUNK_SIZE_LIMIT'
      ? `(!) ${warning.message}` : `[${warning.code}] Warning: ${warning.message}`).join('\n\n'), options);
    const rust = { ...frontend[0], target: 'rust-fixture', crate: 'fixture', code: 'fixture_lint', message: 'fixture' };
    fs.writeFileSync(path.join(root, 'scripts/frontend-warnings-baseline.json'), JSON.stringify(baseline));
    fs.writeFileSync(path.join(root, 'scripts/rust-warnings-baseline.json'), JSON.stringify(makeBaseline([rust], ['rust-fixture'])));
    const save = (target, warnings) => fs.writeFileSync(path.join(root, `artifacts/rust-warnings-fixture-${target}.json`), JSON.stringify({ platform: 'fixture', target, complete: true, warnings }));
    save('frontend', frontend);
    save('rust-fixture', [rust]);
    const run = () => spawnSync(process.execPath, [fileURLToPath(new URL('../rust-warnings-report.mjs', import.meta.url)), 'artifacts', 'report'], {
      cwd: root, encoding: 'utf8', env: { ...process.env, WARNING_ARTIFACTS: '["rust-warnings-fixture-frontend","rust-warnings-fixture-rust-fixture"]' },
    });
    assert.equal(run().status, 0);
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, 'report/rust-warnings-baseline.json'))).warnings, makeBaseline([rust], ['rust-fixture']).warnings);
    assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, 'report/frontend-warnings-baseline.json'))).warnings, makeBaseline(baseline.warnings, ['frontend']).warnings);
    save('rust-fixture', [{ ...rust, message: 'replacement' }]);
    assert.equal(run().status, 1);
    assert.ok(!fs.existsSync(path.join(root, 'report/frontend-warnings-baseline.json')));
    save('rust-fixture', [rust]);
    save('frontend', [{ ...frontend[0], message: 'replacement' }]);
    assert.equal(run().status, 1);
  } finally { fs.rmSync(root, { recursive: true, force: true }); }
});
