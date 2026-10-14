import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { classifyInventory } from './ci-rust-classification.mjs';

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

export function archiveTargets(lane) {
  return lane === 'workspace'
    ? ['--workspace', '--lib', '--exclude', 'codeterm-input-macos', '--exclude', 'codeterm-input-windows']
    : ['--workspace'];
}

function build() {
  const os = process.env.RUNNER_OS;
  const arch = process.env.CI_RUNNER_ARCH;
  const platform = archivePlatform(os, arch);
  const sha = execFileSync('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const receipt = { mode: 'build', os, arch, sha, runner: process.env.RUNNER_NAME, exit: 1, seconds: 0 };
  const start = performance.now();
  fs.mkdirSync('ci-rust-archives', { recursive: true });
  fs.mkdirSync('ci-rust-builds', { recursive: true });
  try {
    for (const lane of ['workspace', 'cli']) {
      const manifest = lane === 'workspace' ? 'src-tauri/Cargo.toml' : 'cli/Cargo.toml';
      const workspace = path.resolve(path.dirname(manifest));
      const metadata = commandJson('cargo', ['metadata', '--locked', '--manifest-path', manifest, '--format-version', '1']);
      const runtime = 'codeterm-embed' + (os === 'Windows' ? '.exe' : '');
      if (lane === 'workspace') {
        execFileSync('cargo', ['build', '--locked', '--manifest-path', manifest, '--bin', 'codeterm-embed'], { stdio: 'inherit' });
        if (!fs.statSync(path.join(metadata.target_directory, 'debug', runtime)).size) throw new Error('Runtime embed sidecar is empty');
      }
      const config = path.resolve('ci-rust-archives', lane + '.toml');
      const include = lane === 'workspace' ? '\narchive.include = [{ path = ' + JSON.stringify('debug/' + runtime) + ', relative-to = "target", on-missing = "error" }]\n' : '\n';
      fs.writeFileSync(config, '[profile.default]\ndefault-filter = "all()"' + include);
      const archive = path.resolve('ci-rust-archives', lane + '.tar.zst');
      const messages = archiveMessages(['nextest', 'archive', '--manifest-path', manifest, ...archiveTargets(lane), '--locked', '--config-file', config, '--archive-file', archive, '--cargo-message-format', 'json']);
      const generated = generatedDirectories(messages);
      if (lane === 'workspace' && !fs.statSync(path.join(metadata.target_directory, 'debug', runtime)).size) throw new Error('Runtime embed sidecar was overwritten while archiving');
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

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) build();
