// Caller: build.sh release-rust; scripts ci-rust-tests.mjs, tests/ci-rust-archives.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { classifyInventory } from './ci-rust-classification.mjs';
import { PATHS, discoverRunner, makeContext, selectRunners } from './test-catalogue.mjs';

const commandJson = (program, args) => JSON.parse(execFileSync(program, args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] }));

export function archivePlatform(os, arch) {
  if (!['macOS', 'Linux', 'Windows'].includes(os) || !['ARM64', 'X64'].includes(arch)) throw new Error('Unsupported archive platform or architecture');
  return os + '-' + arch;
}

export function validateArchiveSuite(document, os, arch, sha) {
  if (document.os !== os || document.arch !== arch || document.sha !== sha) throw new Error('Archive source/platform mismatch');
  if (!Array.isArray(document.rows) || !document.rows.length) throw new Error('Archive has no classified tests');
  return document.rows;
}

export function archiveRunArgs(archive, workspace, extract, config, partition = [], alone = false) {
  return ['--archive-file', archive, '--workspace-remap', workspace, '--extract-to', extract,
    '--extract-overwrite', '--config-file', config, ...partition, '--no-tests=fail',
    '--status-level', 'pass', '--failure-output', 'immediate-final', ...(alone ? ['--test-threads', '1'] : [])];
}

export function archiveMessages(args) {
  let messages = '';
  try {
    messages = execFileSync('cargo', args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] });
    return messages;
  } catch (error) {
    messages = error.stdout?.toString() || '';
    throw error;
  } finally {
    if (process.env.RUST_WARNING_LOG) {
      try { fs.appendFileSync(process.env.RUST_WARNING_LOG, messages); } catch { /* Warning capture cannot fail compilation. */ }
    }
  }
}

export function generatedDirectories(messages) {
  const directories = {};
  for (const line of messages.split('\n').filter(Boolean)) {
    const message = JSON.parse(line);
    if (message.reason === 'build-script-executed') directories[message.package_id] = message.out_dir;
  }
  return directories;
}

export function verifyArchiveBuilds(builds, receipts) {
  if (builds.length < 3 || !['macOS', 'Linux', 'Windows'].every((os) => builds.some((b) => b.os === os))) throw new Error('Missing OS archive build');
  const seen = new Set();
  for (const build of builds) {
    const platform = archivePlatform(build.os, build.arch);
    if (seen.has(platform) || build.exit !== 0) throw new Error('Duplicate or failed archive build');
    seen.add(platform);
  }
  for (const receipt of receipts) {
    const build = builds.find((b) => b.os === receipt.os && b.arch === receipt.arch);
    if (!build || build.sha !== receipt.sha) throw new Error('Shard lacks matching source/OS/architecture build');
  }
  return '| Archive platform | Builder | Build and archive duration |\n| --- | --- | ---: |\n' +
    builds.map((b) => '| ' + archivePlatform(b.os, b.arch) + ' | ' + b.runner + ' | ' + b.seconds.toFixed(1) + ' s |').join('\n');
}

const SHARED_JOB = 'tests.yml:rust-tests';
const NEXTEST_SCOPE = /^\.\/build\.sh nextest ([\w-]+)$/;
const SAFE_NAME = /^[A-Za-z0-9_:.\/-]+$/;
const HOST_PLATFORM = { win32: 'windows', darwin: 'macos', linux: 'linux' }[process.platform] ?? process.platform;

export function releaseRustRunners(catalogue, overlay) {
  return selectRunners(catalogue, overlay, { tier: 'release' }).runners.filter((r) => r.lang === 'rust' && r.kind !== 'guard');
}

export function loadReleaseRust(root = '.') {
  const read = (rel) => JSON.parse(fs.readFileSync(path.join(root, rel), 'utf8'));
  const catalogue = read(PATHS.catalogue);
  const overlay = read(PATHS.overlay);
  const runners = releaseRustRunners(catalogue, overlay);
  const ctx = makeContext(root, catalogue);
  return { runners, overlay, testCounts: Object.fromEntries(runners.map((r) => [r.id, discoverRunner(r, ctx).tests.length])) };
}

const archiveLane = (runner) => (runner.manifest.startsWith('cli/') ? 'cli' : 'workspace');

// One cargo invocation per lane, so target flags are the union over its packages; `--test NAME` stays one unit.
export function archiveTargets(lane, runners) {
  const unplaced = runners.filter((r) => !r.ci?.length);
  if (unplaced.length) throw new Error('Release Rust runners with no CI placement: ' + unplaced.map((r) => r.id).join(', '));
  const members = runners.filter((r) => r.ci.includes(SHARED_JOB) && archiveLane(r) === lane);
  if (!members.length) throw new Error('No release Rust runner archives in the ' + lane + ' lane');
  const units = new Map();
  for (const r of members) {
    const flags = r.cargo_targets ?? ['--lib'];
    for (let i = 0; i < flags.length; i++) {
      const unit = flags[i] === '--test' ? [flags[i], flags[++i]] : [flags[i]];
      units.set(unit.join(' '), unit);
    }
  }
  return [...new Set(members.map((r) => r.package))].flatMap((p) => ['-p', p]).concat([...units.values()].flat());
}

// Groups release Rust runners by build.sh nextest scope; stale and this platform's platform-red markers become exclusions.
// A group whose inventory holds no test passes nextest's no-tests check; one that should hold tests still fails it.
export function releaseRustPlan(runners, overlay, platform = HOST_PLATFORM, only = [], testCounts = {}) {
  const unknown = only.filter((id) => !runners.some((r) => r.id === id));
  if (unknown.length) throw new Error('Not release-tier Rust runners: ' + unknown.join(', '));
  const groups = new Map();
  for (const r of runners.filter((x) => !only.length || only.includes(x.id))) {
    const scope = NEXTEST_SCOPE.exec(r.runner)?.[1];
    if (!scope) throw new Error(r.id + ' is release-tier Rust but does not run through ./build.sh nextest <scope>: ' + r.runner);
    if (!groups.has(scope)) groups.set(scope, { scope, ids: [], budget: 0 });
    const group = groups.get(scope);
    group.ids.push(r.id);
    group.budget = Math.max(group.budget, r.budget_secs);
  }
  return [...groups.values()].map((group) => {
    const skips = [...overlay.stale, ...overlay.platform_red].filter((m) => group.ids.includes(m.runner) && 'binary' in m.target && (!m.platforms || m.platforms.includes(platform)));
    for (const m of skips) if (!SAFE_NAME.test(m.target.binary) || !SAFE_NAME.test(m.target.test)) throw new Error('Unsafe overlay target for ' + m.runner);
    const filterset = skips.map((m) => 'not (binary_id(=' + m.target.binary + ') & test(=' + m.target.test + '))').join(' & ');
    return { ...group, noTests: group.ids.every((id) => testCounts[id] === 0) ? 'pass' : 'fail', filterset };
  });
}

export const formatReleasePlan = (plan) => plan.map((g) => [g.scope, g.ids.join(','), g.budget, g.noTests, g.filterset].join('\t') + '\n').join('');

function build() {
  const os = process.env.RUNNER_OS;
  const arch = process.env.CI_RUNNER_ARCH;
  const platform = archivePlatform(os, arch);
  const sha = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const receipt = { mode: 'build', os, arch, sha, runner: process.env.RUNNER_NAME, exit: 1, seconds: 0 };
  const start = performance.now();
  fs.mkdirSync('ci-rust-archives', { recursive: true });
  fs.mkdirSync('ci-rust-builds', { recursive: true });
  const { runners } = loadReleaseRust();
  try {
    for (const lane of ['workspace', 'cli']) {
      const manifest = lane === 'workspace' ? 'src-tauri/Cargo.toml' : 'cli/Cargo.toml';
      const workspace = path.resolve(path.dirname(manifest));
      const metadata = commandJson('cargo', ['metadata', '--locked', '--manifest-path', manifest, '--format-version', '1']);
      const config = path.resolve('ci-rust-archives', lane + '.toml');
      fs.writeFileSync(config, '[profile.default]\ndefault-filter = "all()"\n');
      const archive = path.resolve('ci-rust-archives', lane + '.tar.zst');
      const messages = archiveMessages(['nextest', 'archive', '--manifest-path', manifest, ...archiveTargets(lane, runners), '--locked', '--config-file', config, '--archive-file', archive, '--cargo-message-format', 'json']);
      const generated = generatedDirectories(messages);
      const extract = path.resolve(process.env.RUNNER_TEMP, 'rust-archive-classify', lane);
      fs.mkdirSync(extract, { recursive: true });
      const inventory = commandJson('cargo-nextest', ['nextest', 'list', '--archive-file', archive, '--workspace-remap', workspace, '--extract-to', extract, '--extract-overwrite', '--ignore-default-filter', '--message-format', 'json']);
      const sources = {};
      for (const suite of Object.values(inventory['rust-suites'])) {
        const pkg = metadata.packages.find((p) => p.id === suite['package-id']);
        const target = pkg?.targets.find((t) => t.name === suite['binary-name'] && (suite.kind === 'lib' ? t.kind.some((k) => ['lib', 'rlib', 'cdylib', 'staticlib'].includes(k)) : t.kind.includes(suite.kind)));
        if (!target) throw new Error('No source root for ' + suite['binary-id']);
        sources[suite['binary-id']] = commandJson('cargo', ['run', '--quiet', '--locked', '--manifest-path', 'scripts/ci-test-classifier/Cargo.toml', '--', target.src_path, ...(generated[pkg.id] ? [generated[pkg.id]] : [])]);
      }
      fs.writeFileSync(path.resolve('ci-rust-archives', lane + '.suite.json'), JSON.stringify({ os, arch, sha, rows: classifyInventory(inventory, sources) }));
    }
    receipt.exit = 0;
  } finally {
    receipt.seconds = (performance.now() - start) / 1000;
    fs.writeFileSync(path.resolve('ci-rust-builds', platform + '.json'), JSON.stringify(receipt, null, 2));
    if (process.env.GITHUB_STEP_SUMMARY) fs.appendFileSync(process.env.GITHUB_STEP_SUMMARY, '\nArchive ' + platform + ': ' + receipt.runner + ', ' + receipt.seconds.toFixed(1) + ' s, exit ' + receipt.exit + '\n');
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv[2] === 'release-plan') {
    const { runners, overlay, testCounts } = loadReleaseRust();
    process.stdout.write(formatReleasePlan(releaseRustPlan(runners, overlay, HOST_PLATFORM, process.argv.slice(3), testCounts)));
  } else build();
}
