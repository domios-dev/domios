// Caller: build.sh ci-tests; scripts tests/ci-rust-tests.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { classifyInventory } from './ci-rust-classification.mjs';
import { overlayExclusions } from './ci-rust-archives.mjs';
import { classifyTarget } from './ci-classifier-cache.mjs';
import { FULL_LANES, testTargetDir } from './rust-test-lanes.mjs';
import { renderedCompilerErrors } from './collect-rust-warnings.mjs';
import { prepareCliServer } from './cli-server-test.mjs';
import { failureDetector, failureEvent, firstFailure } from './ci-first-failure.mjs';

// The catalogue's release tier names every package and target, including path dependencies outside the workspace.
const OVERLAY = JSON.parse(fs.readFileSync(new URL('../tests/catalogue.overlay.json', import.meta.url), 'utf8'));
const [WORKSPACE, CLI] = FULL_LANES;
const INPUT = { lane: 'input', manifest: 'src-tauri/Cargo.toml', targets: ['--no-default-features', '--lib', '-p', 'codeterm-input-macos', '-p', 'codeterm-input-windows'], whole: true };
const EMBEDDINGS = { lane: 'embeddings', manifest: 'src-tauri/Cargo.toml', targets: ['--no-default-features', '--features', 'codeterm-service-core/embeddings,codeterm-mem/embeddings', '-p', 'codeterm-service-core', '-p', 'codeterm-mem', '--lib'] };
// The full suite runs once on one runner; each OS runs only its cfg-gated tests and the native input crates.
export const MODES = { full: [WORKSPACE, CLI], platform: [WORKSPACE, CLI, INPUT], embeddings: [EMBEDDINGS] };
const CI_PLATFORM = { Linux: 'linux', macOS: 'macos', Windows: 'windows' };

// The catalogue overlay's stale and platform-red binary markers apply to CI exactly as to release-rust.
// Only markers whose package the lane builds apply: nextest refuses a binary_id that matches nothing.
export function withOverlayExclusions(scope, overlay, os, lane) {
  const platform = CI_PLATFORM[os];
  if (!platform) throw new Error(`Unknown runner OS ${os}`);
  const packages = new Set(lane.targets.flatMap((arg, index) => (lane.targets[index - 1] === '-p' ? [arg] : [])));
  const exclusions = overlayExclusions(overlay, platform, (m) => packages.has(m.target.binary.split('::')[0]));
  return exclusions ? `(${scope}) & ${exclusions}` : scope;
}
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

export function compile(lane, { execute = execFileSync, writeError = text => process.stderr.write(text) } = {}) {
  let messages = '';
  try {
    messages = execute('cargo-nextest', ['nextest', 'run', '--no-run', '--manifest-path', lane.manifest, ...lane.targets, '--locked', '--cargo-message-format', 'json'],
      { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] });
    return messages;
  } catch (error) {
    messages = error.stdout?.toString() || '';
    writeError(renderedCompilerErrors(messages));
    throw error;
  } finally {
    if (process.env.RUST_WARNING_LOG) {
      try { fs.appendFileSync(process.env.RUST_WARNING_LOG, messages); } catch { /* Warning capture cannot fail the tests. */ }
    }
  }
}

const LIB_KINDS = ['lib', 'rlib', 'cdylib', 'staticlib'];

// A platform lane compiles only the packages whose sources hold cfg-gated tests; the classifier reads sources without a build.
export function platformLane(lane, packages, hasPlatformTests) {
  const args = lane.targets;
  const wanted = args.filter((arg, index) => args[index - 1] === '-p');
  const named = args.filter((arg, index) => args[index - 1] === '--test');
  const allTargets = args.includes('--all-targets');
  const kept = packages.filter((pkg) => wanted.includes(pkg.name) && pkg.targets.some((target) =>
    (allTargets || LIB_KINDS.some((kind) => target.kind.includes(kind)) || (target.kind.includes('test') && named.includes(target.name)))
    && hasPlatformTests(target.src_path)));
  if (!kept.length) return null;
  const keptNames = new Set(kept.map((pkg) => pkg.name));
  const testTargets = new Set(kept.flatMap((pkg) => pkg.targets.filter((target) => target.kind.includes('test')).map((target) => target.name)));
  const targets = [];
  for (let index = 0; index < args.length; index++) {
    const [flag, value] = [args[index], args[index + 1]];
    if (flag === '-p' || flag === '--test') {
      if (flag === '-p' ? keptNames.has(value) : testTargets.has(value)) targets.push(flag, value);
      index++;
    } else targets.push(flag);
  }
  return { ...lane, targets };
}

function narrowToPlatformPackages(lane) {
  const metadata = JSON.parse(execFileSync('cargo', ['metadata', '--locked', '--no-deps', '--manifest-path', lane.manifest, '--format-version', '1'],
    { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] }));
  const stats = { local: 0, store: 0, classified: 0 };
  const started = performance.now();
  // A source that needs its build script's OUT_DIR cannot be read before compiling, so it stays in the lane.
  const hasPlatformTests = (source) => {
    try {
      return classifyTarget(source, undefined, { stats }).some((entry) => entry.platform);
    } catch {
      console.log(`[platform] ${source} needs build output to classify; kept`);
      return true;
    }
  };
  const narrowed = platformLane(lane, metadata.packages, hasPlatformTests);
  console.log(`[platform] ${lane.lane} pre-pass ${((performance.now() - started) / 1000).toFixed(1)} s: ${stats.local} cached here, ${stats.store} from the store, ${stats.classified} classified`);
  return narrowed;
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
    sources[suite['binary-id']] = classifyTarget(target.src_path, generated[pkg.id]);
  }
  return classifyInventory(inventory, sources);
}
const LISTENER = 'test(/startup::rest_api::tests::/)';
const CLI_SERVER = 'binary_id(=codeterm-cli::isolated_server)';
const jsonCommand = (args) => JSON.parse(execFileSync('cargo-nextest', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] }));

export function executionGroups(selected) {
  const integration = selected.filter(test => test.binary === 'codeterm-cli::isolated_server');
  const units = selected.filter(test => test.binary !== 'codeterm-cli::isolated_server');
  return [...[false, true].map((listener) => ({
    listener,
    selected: units.filter((test) => test.name.includes('startup::rest_api::tests::') === listener),
  })), { listener: false, integration: true, selected: integration }].filter((group) => group.selected.length > 0);
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

export async function captureCommand(command, args, logPath, env = process.env, echo = true, onFailure = detail => firstFailure(detail, { env })) {
  const fd = fs.openSync(logPath, 'w');
  let failure;
  const seen = new Set();
  const detect = () => failureDetector(detail => {
    failure ??= Promise.resolve().then(() => onFailure(detail)).catch(() => {});
  }, failureEvent(env), seen);
  const detectors = [];
  try {
    return await new Promise((resolve) => {
      const child = spawn(command, args, { stdio: ['ignore', 'pipe', 'pipe'], env });
      let error;
      child.on('error', (cause) => { error = cause; });
      for (const stream of [child.stdout, child.stderr]) {
        const detector = detect();
        detectors.push(detector);
        stream.on('data', (chunk) => {
          fs.writeSync(fd, chunk);
          if (echo) process.stdout.write(chunk);
          detector.write(chunk);
        });
      }
      child.on('close', async (status, signal) => {
        for (const detector of detectors) detector.end();
        await failure;
        resolve({ status, signal, error });
      });
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
  if (receipts.filter((r) => r.mode === 'embeddings').length !== 1) throw new Error('Expected exactly one embeddings receipt');
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

const tomlConfig = (filter, listener) => `[profile.default]\ndefault-filter = ${JSON.stringify(filter)}\nretries = 0\nfail-fast = false\n${listener ? 'test-threads = 1\n' : ''}slow-timeout = { period = "60s", terminate-after = 5 }\n`;

async function run(mode) {
  const lanes = MODES[mode];
  if (!lanes) throw new Error('usage: node scripts/ci-rust-tests.mjs full|platform|embeddings | verify DIR');
  for (const name of ['CODETERM_CONPTY_DIR', 'CODETERM_PLAN_ID', 'CODETERM_TASK_ID']) delete process.env[name];
  const env = { ...process.env, TEST_CAP_SECS: '1800', CODETERM_CONPTY_TEST_ISOLATION: '1' };
  const os = process.env.RUNNER_OS || { win32: 'Windows', darwin: 'macOS', linux: 'Linux' }[process.platform];
  const plan = timingPlan({ release: process.env.CI_RELEASE_TESTS === 'true', os });
  const start = performance.now();
  const receipt = { mode, os, arch: process.env.RUNNER_ARCH, runner: process.env.RUNNER_NAME, sha: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), executed: 0, exit: 1, seconds: 0 };
  const directory = path.resolve('ci-rust-receipts');
  fs.mkdirSync(directory, { recursive: true });
  const targetEnv = { ...env };
  try {
    for (const planned of lanes) {
      const lane = mode === 'platform' && !planned.whole ? narrowToPlatformPackages(planned) : planned;
      if (!lane) { console.log(`[platform] ${planned.lane}: no package holds cfg-gated tests; nothing to compile`); continue; }
      if (lane !== planned) console.log(`[platform] ${lane.lane}: compiling ${lane.targets.filter((arg, index) => lane.targets[index - 1] === '-p').join(', ')}`);
      const targetDir = testTargetDir(lane, { env: targetEnv, mode });
      if (targetDir) process.env.CARGO_TARGET_DIR = env.CARGO_TARGET_DIR = targetDir;
      console.log(`[rust-tests] ${mode}/${lane.lane} target=${targetDir}`);
      if (lane.lane === 'cli') {
        env.DOMIOS_CLI_TEST_SERVER = prepareCliServer({
          cliTarget: targetDir, serverTarget: testTargetDir(WORKSPACE, { env: targetEnv, mode }), env,
        });
      }
      const messages = compile(lane);
      const compiled = messages.split('\n').filter(Boolean).flatMap(line => {
        try { const message = JSON.parse(line); return message.reason === 'compiler-artifact' && !message.fresh ? [message] : []; } catch { return []; }
      }).length;
      console.log(`[rust-tests] ${mode}/${lane.lane} compiled=${compiled}`);
      const config = path.join(directory, `${mode}-${lane.lane}.toml`);
      fs.writeFileSync(config, tomlConfig('all()', false));
      let scope = mode === 'embeddings' ? 'package(=codeterm-mem) | (package(=codeterm-service-core) & test(/^mem::/))' : 'all()';
      fs.writeFileSync(config, tomlConfig(scope, false));
      if (mode === 'platform' && !lane.whole) {
        const rows = classify(lane, messages, config);
        for (const row of rows.filter((r) => r.ignored)) console.log(`[explicit ignore on ${receipt.runner}] ${row.id}: ${row.reason}`);
        scope = platformFilter(rows);
        if (scope === 'none()') continue;
      }
      scope = withOverlayExclusions(scope, OVERLAY, os, lane);
      fs.writeFileSync(config, tomlConfig(scope, false));
      const listed = jsonCommand(['nextest', 'list', '--manifest-path', lane.manifest, ...lane.targets, '--locked', '--config-file', config, '--message-format', 'json']);
      const selected = Object.values(listed['rust-suites']).flatMap((suite) => Object.entries(suite.testcases)
        .filter(([, test]) => test['filter-match'].status === 'matches')
        .map(([name]) => ({ binary: suite['binary-id'], name })));
      // Listener lifecycle cases need a process boundary without concurrent fork holders (#620).
      for (const { listener, integration, selected: group } of executionGroups(selected)) {
        const unitFilter = listener ? LISTENER : `not (${LISTENER})`;
        const category = integration ? CLI_SERVER : lane.lane === 'cli' ? `${unitFilter} & not (${CLI_SERVER})` : unitFilter;
        const filter = `(${scope}) & (${category})`;
        fs.writeFileSync(config, tomlConfig(filter, listener));
        const log = path.join(directory, `${mode}-${lane.lane}-${integration ? 'integration' : listener ? 'listener' : 'normal'}.log`);
        const result = await captureCommand('bash', ['scripts/test-capped.sh', '--exec', 'cargo-nextest', ...runArgs(lane, config)], log, env);
        const report = fs.readFileSync(log, 'utf8');
        requireTestSuccess(result, report, `Rust tests failed on ${receipt.runner}`);
        receipt.executed += executedCount(report);
        if (integration) continue;
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
    if (mode === 'embeddings') {
      const lane = lanes[0];
      const config = path.join(directory, 'embedding-parity.toml');
      fs.writeFileSync(config, tomlConfig('package(=codeterm-mem) & test(=embedding_model::tests::candle_embedding_matches_fastembed_golden)', false));
      const log = path.join(directory, 'embedding-parity.log');
      const result = await captureCommand('bash', ['scripts/test-capped.sh', '--exec', 'cargo-nextest',
        ...runArgs(lane, config), '--run-ignored', 'only'], log, env);
      const report = fs.readFileSync(log, 'utf8');
      requireTestSuccess(result, report, 'Embedding golden parity');
      const count = executedCount(report);
      if (count !== 1) throw new Error(`Expected one embedding parity oracle, got ${count}`);
      receipt.executed += count;
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
