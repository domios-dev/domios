import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { archivePlatform, validateArchiveSuite, archiveRunArgs, verifyArchiveBuilds } from './ci-rust-archives.mjs';

const manifests = ['src-tauri/Cargo.toml', 'cli/Cargo.toml'];
const jsonCommand = (args) => JSON.parse(execFileSync('cargo-nextest', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] }));
const regexEscape = (value) => value.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&');

export { classifyInventory } from './ci-rust-classification.mjs';

export function filterFor(rows, mode) {
  const groups = new Map();
  for (const row of rows) {
    const key = JSON.stringify([row.package, row.binary]);
    if (!groups.has(key)) groups.set(key, []);
    if (mode === 'shared' ? row.platform || row.ignored : row.platform && !row.ignored) groups.get(key).push(regexEscape(row.name));
  }
  return [...groups].map(([key, names]) => {
    const [pkg, binary] = JSON.parse(key);
    if (!/^[\w-]+$/.test(pkg) || !/^[\w-]+$/.test(binary)) throw new Error('Unsupported Cargo target name');
    const gated = names.length ? `test(/^(?:${names.join('|')})$/)` : 'none()';
    return `(package(=${pkg}) & binary(=${binary}) & ${mode === 'shared' ? `not (${gated})` : gated})`;
  }).join(' | ') || 'none()';
}

export function verifyCoverage(receipts, shards = 12) {
  if (receipts.length !== shards + 3) throw new Error('Missing shard or platform receipt');
  const platforms = receipts.filter((r) => r.mode === 'platform');
  if (new Set(platforms.map((r) => r.os)).size !== 3 || !['Linux', 'macOS', 'Windows'].every((os) => platforms.some((r) => r.os === os))) throw new Error('Missing native OS');
  if (new Set(receipts.map((r) => r.sha)).size !== 1) throw new Error('Mixed source SHAs');
  if (receipts.some((r) => r.exit !== 0)) throw new Error('Failed test job');
  const expected = platforms[0].all.filter((r) => !r.platform && !r.ignored).map((r) => r.id).sort();
  if (!expected.length) throw new Error('Empty shared inventory');
  for (const receipt of receipts) {
    if (receipt.all.some((row) => row.ignored && !row.reason?.trim())) throw new Error('Unexplained ignored test');
    const shared = receipt.all.filter((r) => !r.platform && !r.ignored).map((r) => r.id).sort();
    if (JSON.stringify(shared) !== JSON.stringify(expected)) throw new Error(`Shared inventory differs on ${receipt.runner}`);
    if (receipt.mode === 'platform') {
      const native = receipt.all.filter((r) => r.platform && !r.ignored).map((r) => r.id).sort();
      if (JSON.stringify([...receipt.selected].sort()) !== JSON.stringify(native)) throw new Error('Incomplete native coverage');
    }
  }
  const workers = receipts.filter((r) => r.mode === 'shared');
  if (workers.length !== shards || new Set(workers.map((r) => r.shard)).size !== shards || workers.some((r) => r.shard < 1 || r.shard > shards)) throw new Error('Duplicate or invalid shard');
  const actual = workers.flatMap((r) => r.selected).sort();
  if (JSON.stringify(actual) !== JSON.stringify(expected)) throw new Error('Shared tests omitted, duplicated, or misclassified');
  const ignored = platforms.flatMap((r) => r.all.filter((t) => t.ignored).map((t) => `- ${r.os}: ${t.id}: ${t.reason}`));
  const reasons = new Map();
  for (const receipt of platforms) for (const row of receipt.all.filter((r) => r.ignored)) {
    if (!reasons.has(row.reason)) reasons.set(row.reason, { entries: 0, tests: new Set() });
    const counts = reasons.get(row.reason);
    counts.entries++;
    counts.tests.add(row.id);
  }
  const reasonCounts = [...reasons].map(([reason, counts]) => `- ${reason}: ${counts.tests.size} distinct tests, ${counts.entries} OS/test entries`).join('\n');
  const audits = receipts.flatMap((r) => (r.timingAudits || []).filter((a) => a.candidates.length).map((a) => `\n${r.runner}, ${a.parallelLog}:\n\n${a.report}`)).join('\n');
  return receipts.map((r) => `| ${r.mode === 'shared' ? `shard ${r.shard}/${shards}` : r.os} | ${r.runner} | ${r.os} | ${r.selected.length} | ${r.seconds.toFixed(1)} s |`).join('\n') + (ignored.length ? `\n\nIgnore counts per source reason:\n${reasonCounts}\n\nExplicit source ignores (${ignored.length} OS/test entries):\n${ignored.join('\n')}` : '') + (audits ? `\n\nParallel and alone timing audits:\n${audits}` : '');
}

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

async function run(mode, shard, total) {
  if (!['shared', 'platform'].includes(mode) || !Number.isInteger(shard) || !Number.isInteger(total) || shard < 1 || shard > total) throw new Error('Invalid mode/partition');
  delete process.env.CODETERM_CONPTY_DIR;
  delete process.env.CODETERM_PLAN_ID;
  delete process.env.CODETERM_TASK_ID;
  process.env.CODETERM_CONPTY_TEST_ISOLATION = '1';
  const start = performance.now();
  const receipt = { mode, shard, arch: process.env.CI_RUNNER_ARCH, runner: process.env.RUNNER_NAME, os: process.env.RUNNER_OS, sha: execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim(), all: [], selected: [], timingAudits: [], exit: 1, seconds: 0 };
  fs.mkdirSync('ci-rust-receipts', { recursive: true });
  try {
    for (const required of ['scripts/check-slow-unit-tests.mjs', 'scripts/slow-unit-tests.json']) {
      if (!fs.existsSync(required)) throw new Error(`Anvil dependency not landed: ${required}`);
    }
    for (const manifest of manifests) {
      archivePlatform(receipt.os, receipt.arch);
      const lane = manifest.startsWith('src-tauri') ? 'workspace' : 'cli';
      const workspace = path.resolve(path.dirname(manifest));
      const archive = path.resolve('ci-rust-archives', lane + '.tar.zst');
      const extract = path.resolve(process.env.RUNNER_TEMP, 'rust-archive-run', lane);
      fs.mkdirSync(extract, { recursive: true });
      const rows = validateArchiveSuite(JSON.parse(fs.readFileSync(path.resolve('ci-rust-archives', lane + '.suite.json'), 'utf8')), receipt.os, receipt.arch, receipt.sha);
      for (const row of rows.filter((r) => r.ignored)) console.log(`[explicit ignore on ${receipt.runner}] ${row.id}: ${row.reason}`);
      receipt.all.push(...rows);
      const filter = filterFor(rows, mode);
      const partition = mode === 'shared' ? ['--partition', `hash:${shard}/${total}`] : [];
      const config = path.resolve('ci-rust-receipts', 'selection.toml');
      fs.writeFileSync(config, `[profile.default]\ndefault-filter = ${JSON.stringify(filter)}\nretries = 0\nslow-timeout = { period = "60s", terminate-after = 5 }\n`);
      const selected = jsonCommand(['nextest', 'list', '--archive-file', archive, '--workspace-remap', workspace, '--extract-to', extract, '--extract-overwrite', '--config-file', config, ...partition, '--message-format', 'json']);
      const selectedTests = [];
      for (const suite of Object.values(selected['rust-suites'])) {
        for (const [name, test] of Object.entries(suite.testcases)) {
          if (test['filter-match'].status === 'matches') {
            receipt.selected.push(`${suite['binary-id']}::${name}`);
            selectedTests.push({ id: `${suite['binary-id']}::${name}`, binary: suite['binary-id'], name });
          }
        }
      }
      // Listener lifecycle cases need a process boundary without concurrent fork holders (#620).
      for (const { listener, selected: groupTests } of executionGroups(selectedTests)) {
        const isolation = 'test(/startup::rest_api::tests::/)';
        const groupFilter = `(${filter}) & ${listener ? isolation : `not (${isolation})`}`;
        fs.writeFileSync(config, `[profile.default]\ndefault-filter = ${JSON.stringify(groupFilter)}\nretries = 0\n${listener ? 'test-threads = 1\n' : ''}slow-timeout = { period = "60s", terminate-after = 5 }\n`);
        console.log(`[${mode} ${shard}/${total} runner=${receipt.runner}] ${manifest} listener=${listener}`);
        const log = path.resolve('ci-rust-receipts', `${mode}-${mode === 'shared' ? shard : receipt.os}-${manifest.startsWith('src-tauri') ? 'workspace' : 'cli'}-${listener ? 'listener' : 'normal'}.log`);
        const result = await captureCommand('bash', ['scripts/test-capped.sh', '--exec', 'cargo-nextest', 'nextest', 'run', ...archiveRunArgs(archive, workspace, extract, config, partition)], log, { ...process.env, TEST_CAP_SECS: '1800', CODETERM_CONPTY_TEST_ISOLATION: '1' });
        if (result.error || result.status !== 0) throw new Error(`Tests failed on ${receipt.runner}, ${mode} ${shard}/${total}: ${result.error || result.status}`);
        const guard = ['scripts/check-slow-unit-tests.mjs', log, '--base', process.env.SLOW_UNIT_TESTS_BASE || 'origin/develop'];
        const candidatePath = log.replace(/\.log$/, '.candidates');
        execFileSync(process.execPath, [...guard, '--candidates', candidatePath], { stdio: 'inherit' });
        const candidates = JSON.parse(fs.readFileSync(candidatePath, 'utf8'));
        const exactFilter = candidateFilter(candidates, groupTests);
        let aloneLog;
        if (exactFilter) {
          aloneLog = log.replace(/\.log$/, '.alone.log');
          fs.writeFileSync(config, `[profile.default]\ndefault-filter = ${JSON.stringify(`(${groupFilter}) & (${exactFilter})`)}\nretries = 0\nslow-timeout = { period = "60s", terminate-after = 5 }\n`);
          const alone = await captureCommand('bash', ['scripts/test-capped.sh', '--exec', 'cargo-nextest', 'nextest', 'run', ...archiveRunArgs(archive, workspace, extract, config, [], true)], aloneLog, { ...process.env, TEST_CAP_SECS: '1800', CODETERM_CONPTY_TEST_ISOLATION: '1' });
          if (alone.error || alone.status !== 0) throw new Error(`Candidate rerun failed on ${receipt.runner}: ${alone.error || alone.status}`);
        }
        const auditLog = log.replace(/\.log$/, '.audit.log');
        const audit = await captureCommand(process.execPath, [...guard, ...(aloneLog ? ['--alone', aloneLog] : [])], auditLog);
        receipt.timingAudits.push({ parallelLog: path.basename(log), aloneLog: aloneLog && path.basename(aloneLog), candidates: candidates.entries, report: fs.readFileSync(auditLog, 'utf8') });
        if (audit.error || audit.status !== 0) throw new Error(`Alone-measured timing ratchet failed on ${receipt.runner}: ${audit.error || audit.status}`);
      }
    }
    receipt.exit = 0;
  } finally {
    receipt.seconds = (performance.now() - start) / 1000;
    const output = path.join('ci-rust-receipts', `${mode}-${mode === 'shared' ? shard : receipt.os}.json`);
    fs.writeFileSync(output, JSON.stringify(receipt, null, 2));
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, `\n${mode} ${shard}/${total}: **${receipt.runner}** (${receipt.os}), ${receipt.seconds.toFixed(1)} s, ${receipt.selected.length} tests, exit ${receipt.exit}\n`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv[2] === 'verify') {
    const receipts = fs.readdirSync(process.argv[3]).filter((f) => f.endsWith('.json')).map((f) => JSON.parse(fs.readFileSync(path.join(process.argv[3], f))));
    const table = `| Job | Runner | OS | Tests | Duration |\n| --- | --- | --- | ---: | ---: |\n${verifyCoverage(receipts)}\n`;
    const builds = fs.readdirSync(process.argv[4]).filter((f) => f.endsWith('.json')).map((f) => JSON.parse(fs.readFileSync(path.join(process.argv[4], f))));
    const buildTable = verifyArchiveBuilds(builds, receipts);
    console.log(buildTable + '\n\n' + table);
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, buildTable + '\n\n' + table);
  } else await run(process.argv[2], Number(process.argv[3] || 1), Number(process.argv[4] || 1));
}
