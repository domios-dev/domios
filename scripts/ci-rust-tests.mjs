// Caller: build.sh ci-tests; scripts tests/ci-rust-tests.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { classifyInventory } from './ci-rust-classification.mjs';

const WORKSPACE = { lane: 'workspace', manifest: 'src-tauri/Cargo.toml', targets: ['--workspace', '--lib', '--exclude', 'codeterm-input-macos', '--exclude', 'codeterm-input-windows'] };
const CLI = { lane: 'cli', manifest: 'cli/Cargo.toml', targets: ['--workspace'] };
const INPUT = { lane: 'input', manifest: 'src-tauri/Cargo.toml', targets: ['--lib', '-p', 'codeterm-input-macos', '-p', 'codeterm-input-windows'], whole: true };
// The full suite runs once on one runner; each OS runs only its cfg-gated tests and the native input crates.
export const MODES = { full: [WORKSPACE, CLI], platform: [WORKSPACE, CLI, INPUT] };
const regexEscape = (value) => value.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&');

export function platformFilter(rows) {
  const groups = new Map();
  for (const row of rows) {
    const key = JSON.stringify([row.package, row.binary]);
    if (!groups.has(key)) groups.set(key, []);
    if (row.platform && !row.ignored) groups.get(key).push(regexEscape(row.name));
  }
  const filters = [...groups].filter(([, names]) => names.length).map(([key, names]) => {
    const [pkg, binary] = JSON.parse(key);
    if (!/^[\w-]+$/.test(pkg) || !/^[\w-]+$/.test(binary)) throw new Error('Unsupported Cargo target name');
    return `(package(=${pkg}) & binary(=${binary}) & test(/^(?:${names.join('|')})$/))`;
  });
  return filters.join(' | ') || 'none()';
}

export function generatedDirectories(messages) {
  const directories = {};
  for (const line of messages.split('\n').filter(Boolean)) {
    let message;
    try { message = JSON.parse(line); } catch { continue; }
    if (message.reason === 'build-script-executed') directories[message.package_id] = message.out_dir;
  }
  return directories;
}

function compile(lane) {
  let messages = '';
  try {
    messages = execFileSync('cargo-nextest', ['nextest', 'run', '--no-run', '--manifest-path', lane.manifest, ...lane.targets, '--locked', '--cargo-message-format', 'json'],
      { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] });
    return messages;
  } catch (error) {
    messages = error.stdout?.toString() || '';
    throw error;
  } finally {
    if (process.env.RUST_WARNING_LOG) {
      try { fs.appendFileSync(process.env.RUST_WARNING_LOG, messages); } catch { /* Warning capture cannot fail the tests. */ }
    }
  }
}

function classify(lane, messages, config) {
  const cargoJson = (args) => JSON.parse(execFileSync('cargo', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] }));
  const metadata = cargoJson(['metadata', '--locked', '--manifest-path', lane.manifest, '--format-version', '1']);
  const generated = generatedDirectories(messages);
  const inventory = jsonCommand(['nextest', 'list', '--manifest-path', lane.manifest, ...lane.targets, '--locked', '--config-file', config, '--ignore-default-filter', '--message-format', 'json']);
  const sources = {};
  for (const suite of Object.values(inventory['rust-suites'])) {
    const pkg = metadata.packages.find((p) => p.id === suite['package-id']);
    const target = pkg?.targets.find((t) => t.name === suite['binary-name'] && (suite.kind === 'lib' ? t.kind.some((k) => ['lib', 'rlib', 'cdylib', 'staticlib'].includes(k)) : t.kind.includes(suite.kind)));
    if (!target) throw new Error('No source root for ' + suite['binary-id']);
    sources[suite['binary-id']] = cargoJson(['run', '--quiet', '--locked', '--manifest-path', 'scripts/ci-test-classifier/Cargo.toml', '--', target.src_path, ...(generated[pkg.id] ? [generated[pkg.id]] : [])]);
  }
  return classifyInventory(inventory, sources);
}
const LISTENER = 'test(/startup::rest_api::tests::/)';
const jsonCommand = (args) => JSON.parse(execFileSync('cargo-nextest', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] }));

export function executionGroups(selected) {
  return [false, true].map((listener) => ({
    listener,
    selected: selected.filter((test) => test.name.includes('startup::rest_api::tests::') === listener),
  })).filter((group) => group.selected.length > 0);
}

export function candidateFilter(document, selected) {
  if (!Array.isArray(document.entries)) throw new Error('Missing timing candidate entries');
  const allowed = new Set(selected.map((test) => JSON.stringify([test.binary, test.name])));
  const seen = new Set();
  return document.entries.map(({ binary, test }) => {
    if (typeof binary !== 'string' || typeof test !== 'string' ||
        !/^[A-Za-z0-9_:.\/-]+$/.test(binary) || !/^[A-Za-z0-9_:.\/-]+$/.test(test)) throw new Error('Unsafe timing candidate identity');
    const key = JSON.stringify([binary, test]);
    if (!allowed.has(key)) throw new Error(`Timing candidate outside selected subgroup: ${binary} ${test}`);
    if (seen.has(key)) throw new Error('Duplicate timing candidate');
    seen.add(key);
    return `(binary_id(=${binary}) & test(=${test}))`;
  }).join(' | ');
}

export function runArgs({ manifest, targets }, config, alone = false) {
  return ['nextest', 'run', '--manifest-path', manifest, ...targets, '--locked', '--config-file', config, '--no-tests=fail',
    '--status-level', 'pass', '--failure-output', 'immediate-final', ...(alone ? ['--test-threads', '1'] : [])];
}

export function executedCount(report) {
  const text = report.replace(/\x1b\[[0-9;]*m/g, '');
  return [...text.matchAll(/^\s*Summary\s+\[[^\]]+\]\s+(\d+)\s+tests? run/gm)].reduce((sum, match) => sum + Number(match[1]), 0);
}

export async function captureCommand(command, args, logPath, env = process.env, echo = true) {
  const fd = fs.openSync(logPath, 'w');
  try {
    return await new Promise((resolve) => {
      const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'], env });
      let error;
      child.on('error', (cause) => { error = cause; });
      for (const stream of [child.stdout, child.stderr]) stream.on('data', (chunk) => {
        fs.writeSync(fd, chunk);
        if (echo) process.stdout.write(chunk);
      });
      child.on('close', (status, signal) => resolve({ status, signal, error }));
    });
  } finally {
    fs.closeSync(fd);
  }
}

// Serial confirmation costs longer than the suite, so release runs and non-baseline OSes report parallel timings only.
export function timingPlan({ release, os }) {
  return release || os !== 'macOS' ? { confirm: false, policy: 'advisory' } : { confirm: true, policy: 'enforce' };
}

export function timingAuditArgs(guard, aloneLog, policy) {
  return [...guard, ...(aloneLog ? ['--alone', aloneLog] : []), '--policy', policy];
}

export function requireTestSuccess(result, report, label, { warn = console.log, summary = process.env.GITHUB_STEP_SUMMARY } = {}) {
  const output = report.replace(/\x1b\[[0-9;]*m/g, '');
  const failedTest = /^\s*(?:FAIL|TIMEOUT)\s+\[/m.test(output) || /\b[1-9]\d* (?:failed|timed out)\b/.test(output);
  if (result.error || result.signal || result.status !== 0 || failedTest) {
    throw new Error(`${label}: ${result.error || result.signal || (failedTest ? 'failed or timed-out test' : result.status)}`);
  }
  const leaks = output.split('\n').filter(line => /^\s*LEAK\s+\[/.test(line) || /\b[1-9]\d* leaky\b/.test(line));
  if (leaks.length) {
    const warning = `::warning::Nextest leaky pass (${label}): ${leaks.map(line => line.trim()).join('; ')}`;
    warn(warning);
    if (summary) fs.appendFileSync(summary, `\n${warning}\n`);
  }
}

export function verifyReceipts(receipts) {
  if (receipts.filter((r) => r.mode === 'full').length !== 1) throw new Error('Expected exactly one full-suite receipt');
  for (const os of ['macOS', 'Linux', 'Windows']) {
    if (receipts.filter((r) => r.mode === 'platform' && r.os === os).length !== 1) throw new Error(`Expected one ${os} platform receipt`);
  }
  if (new Set(receipts.map((r) => r.sha)).size !== 1) throw new Error('Mixed source SHAs');
  for (const receipt of receipts) {
    if (receipt.exit !== 0) throw new Error(`Failed Rust ${receipt.mode} tests on ${receipt.runner}`);
    if (!(receipt.executed > 0)) throw new Error(`No executed ${receipt.mode} tests on ${receipt.runner}`);
  }
  return '| Suite | Runner | OS | Arch | Executed | Duration |\n| --- | --- | --- | --- | ---: | ---: |\n' +
    receipts.map((r) => `| ${r.mode} | ${r.runner} | ${r.os} | ${r.arch} | ${r.executed} | ${r.seconds.toFixed(1)} s |`).join('\n');
}

// Each workspace keeps its own warm target outside the checkout, so clean checkouts never erase it.
export function laneTargetDir(manifest, root) {
  return root ? path.join(root, path.dirname(manifest).replace(/[\/]/g, '-')) : undefined;
}

const tomlConfig = (filter, listener) => `[profile.default]\ndefault-filter = ${JSON.stringify(filter)}\nretries = 0\nfail-fast = false\n${listener ? 'test-threads = 1\n' : ''}slow-timeout = { period = "60s", terminate-after = 5 }\n`;

async function run(mode) {
  const lanes = MODES[mode];
  if (!lanes) throw new Error('usage: node scripts/ci-rust-tests.mjs full|platform | verify DIR');
  for (const name of ['CODETERM_CONPTY_DIR', 'CODETERM_PLAN_ID', 'CODETERM_TASK_ID']) delete process.env[name];
  const env = { ...process.env, TEST_CAP_SECS: '1800', CODETERM_CONPTY_TEST_ISOLATION: '1' };
  const os = process.env.RUNNER_OS;
  const plan = timingPlan({ release: process.env.CI_RELEASE_TESTS === 'true', os });
  const start = performance.now();
  const receipt = { mode, os, arch: process.env.RUNNER_ARCH, runner: process.env.RUNNER_NAME, sha: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), executed: 0, exit: 1, seconds: 0 };
  const directory = path.resolve('ci-rust-receipts');
  fs.mkdirSync(directory, { recursive: true });
  try {
    for (const lane of lanes) {
      const targetDir = laneTargetDir(lane.manifest, process.env.CI_TARGET_ROOT);
      if (targetDir) process.env.CARGO_TARGET_DIR = env.CARGO_TARGET_DIR = targetDir;
      if (lane.lane === 'workspace') execFileSync('cargo', ['build', '--locked', '--manifest-path', lane.manifest, '--bin', 'codeterm-embed'], { stdio: 'inherit' });
      const messages = compile(lane);
      const config = path.join(directory, `${mode}-${lane.lane}.toml`);
      fs.writeFileSync(config, tomlConfig('all()', false));
      let scope = 'all()';
      if (mode === 'platform' && !lane.whole) {
        const rows = classify(lane, messages, config);
        for (const row of rows.filter((r) => r.ignored)) console.log(`[explicit ignore on ${receipt.runner}] ${row.id}: ${row.reason}`);
        scope = platformFilter(rows);
        if (scope === 'none()') continue;
        fs.writeFileSync(config, tomlConfig(scope, false));
      }
      const listed = jsonCommand(['nextest', 'list', '--manifest-path', lane.manifest, ...lane.targets, '--locked', '--config-file', config, '--message-format', 'json']);
      const selected = Object.values(listed['rust-suites']).flatMap((suite) => Object.entries(suite.testcases)
        .filter(([, test]) => test['filter-match'].status === 'matches')
        .map(([name]) => ({ binary: suite['binary-id'], name })));
      // Listener lifecycle cases need a process boundary without concurrent fork holders (#620).
      for (const { listener, selected: group } of executionGroups(selected)) {
        const filter = `(${scope}) & ${listener ? LISTENER : `not (${LISTENER})`}`;
        fs.writeFileSync(config, tomlConfig(filter, listener));
        const log = path.join(directory, `${mode}-${lane.lane}-${listener ? 'listener' : 'normal'}.log`);
        const result = await captureCommand('bash', ['scripts/test-capped.sh', '--exec', 'cargo-nextest', ...runArgs(lane, config)], log, env);
        const report = fs.readFileSync(log, 'utf8');
        requireTestSuccess(result, report, `Rust tests failed on ${receipt.runner}`);
        receipt.executed += executedCount(report);
        const guard = ['scripts/check-slow-unit-tests.mjs', log, '--base', process.env.SLOW_UNIT_TESTS_BASE || 'origin/develop'];
        let aloneLog;
        if (plan.confirm) {
          const candidatePath = log.replace(/\.log$/, '.candidates');
          execFileSync(process.execPath, [...guard, '--candidates', candidatePath], { stdio: 'inherit' });
          const exact = candidateFilter(JSON.parse(fs.readFileSync(candidatePath, 'utf8')), group);
          if (exact) {
            aloneLog = log.replace(/\.log$/, '.alone.log');
            fs.writeFileSync(config, tomlConfig(`(${filter}) & (${exact})`, false));
            const alone = await captureCommand('bash', ['scripts/test-capped.sh', '--exec', 'cargo-nextest', ...runArgs(lane, config, true)], aloneLog, env);
            requireTestSuccess(alone, fs.readFileSync(aloneLog, 'utf8'), `Candidate rerun failed on ${receipt.runner}`);
          }
        }
        const audit = await captureCommand(process.execPath, timingAuditArgs(guard, aloneLog, plan.policy), log.replace(/\.log$/, '.audit.log'));
        if (audit.error || audit.status !== 0) throw new Error(`Slow unit ratchet failed on ${receipt.runner}: ${audit.error || audit.status}`);
      }
    }
    if (!receipt.executed) throw new Error('No Rust tests executed');
    receipt.exit = 0;
  } finally {
    receipt.seconds = (performance.now() - start) / 1000;
    fs.writeFileSync(path.join(directory, `rust-tests-${mode}-${os}.json`), JSON.stringify(receipt, null, 2));
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\nRust ${mode} tests: **${receipt.runner}** (${os} ${receipt.arch}), ${receipt.executed} executed, ${receipt.seconds.toFixed(1)} s, exit ${receipt.exit}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv[2] === 'verify') {
    const dir = process.argv[3];
    const receipts = fs.readdirSync(dir).filter((f) => f.endsWith('.json')).map((f) => JSON.parse(fs.readFileSync(path.join(dir, f))));
    const table = verifyReceipts(receipts);
    console.log(table);
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n${table}\n`);
  } else await run(process.argv[2]);
}
