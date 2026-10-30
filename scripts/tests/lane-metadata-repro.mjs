// Caller: scripts/tests/lane-check.test.mjs; standalone two-worktree cache reproduction.
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, copyFileSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, utimesSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
const scripts = path.resolve(import.meta.dirname, '..');

export function reproduceLaneMetadata({ isolate = true } = {}) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'lane metadata-'));
  const repository = path.join(root, 'repository');
  mkdirSync(repository);
  const git = (...args) => execFileSync('git', args, { cwd: repository, encoding: 'utf8', stdio: 'pipe' });
  const write = (file, text) => {
    mkdirSync(path.dirname(file), { recursive: true });
    writeFileSync(file, text);
  };
  try {
    git('init');
    git('config', 'user.email', 'fixture@example.test');
    git('config', 'user.name', 'Fixture');
    git('config', 'core.autocrlf', 'false');
    write(path.join(repository, 'src-tauri/Cargo.toml'), '[workspace]\nmembers=["consumer","types"]\nresolver="2"\n');
    write(path.join(repository, 'src-tauri/consumer/Cargo.toml'), '[package]\nname="codeterm-fixture"\nversion="0.1.0"\nedition="2021"\n[dependencies]\nfixture-types={path="../types"}\nfixture-shared="0.1.0"\n');
    write(path.join(repository, 'src-tauri/types/Cargo.toml'), '[package]\nname="fixture-types"\nversion="0.1.0"\nedition="2021"\n');
    write(path.join(repository, 'src-tauri/types/src/lib.rs'), 'pub fn first() {}\n');
    write(path.join(repository, 'src-tauri/consumer/src/lib.rs'), 'pub fn check() { fixture_types::first(); fixture_shared::shared(); }\n');
    write(path.join(repository, 'vendor/fixture-shared/Cargo.toml'), '[package]\nname="fixture-shared"\nversion="0.1.0"\nedition="2021"\n[workspace]\n');
    write(path.join(repository, '.cargo/config.toml'), `[source.crates-io]\nreplace-with="fixture"\n[source.fixture]\ndirectory=${JSON.stringify(path.join(repository, 'vendor').replaceAll('\\', '/'))}\n`);
    write(path.join(repository, 'vendor/fixture-shared/.cargo-checksum.json'), '{"files":{},"package":null}');
    write(path.join(repository, 'vendor/fixture-shared/src/lib.rs'), 'pub fn shared() {}\n');
    for (const name of ['lane-check.sh', 'lane-check-run.py', 'cargo-build-gc.py', 'incremental-retention.py', 'lane-build-dir.mjs', 'lane-native-wrapper.mjs', 'lane-rustc-wrapper.c', 'lane-rustc-wrapper.sh']) {
      const target = path.join(repository, 'scripts', name);
      mkdirSync(path.dirname(target), { recursive: true });
      copyFileSync(path.join(scripts, name), target);
      if (name === 'lane-check-run.py') {
        const marker = 'def read_cli(args):';
        const runner = readFileSync(target, 'utf8');
        assert.equal(runner.split(marker).length, 2);
        writeFileSync(target, runner.replace(marker, marker + '\n    raise AssertionError("metadata fixture must never query a live Domios CLI")'));
      }
      if (name.endsWith('.sh')) chmodSync(target, 0o755);
    }
    git('add', '.');
    git('commit', '-qm', 'fixture');
    const lanes = ['a', 'b'].map(name => path.join(root, name));
    for (const lane of lanes) git('worktree', 'add', '--detach', lane);
    for (const [index, lane] of lanes.entries()) {
      const item = index ? 'second' : 'first';
      write(path.join(lane, 'src-tauri/types/src/lib.rs'), `pub fn ${item}() {}\n`);
      write(path.join(lane, 'src-tauri/consumer/src/lib.rs'), `pub fn check() { fixture_types::${item}(); fixture_shared::shared(); }\n`);
      for (const relative of ['types/src/lib.rs', 'consumer/src/lib.rs']) {
        utimesSync(path.join(lane, 'src-tauri', relative), new Date('2000-01-01'), new Date('2000-01-01'));
      }
    }
    const inherited = { ...process.env };
    for (const key of Object.keys(inherited)) if (/^(CODETERM_|DOMIOS_|CT_|BUILD_NOTIFY_|DEV_NOTIFY_)/.test(key)) delete inherited[key];
    const env = { ...inherited, CODETERM_RUST_CACHE_ROOT: path.join(root, 'cache'), CARGO_HOME: path.join(root, 'cargo-home'), CARGO_TERM_COLOR: 'never', CARGO_TERM_QUIET: 'false', CARGO_INCREMENTAL: '0', CARGO_NET_OFFLINE: 'true', RUSTC_WRAPPER: '', RUSTC_WORKSPACE_WRAPPER: '' };
    mkdirSync(env.CARGO_HOME);
    delete env.CARGO_TARGET_DIR;
    delete env.CARGO_BUILD_TARGET_DIR;
    const runs = [];
    for (const index of [0, 1, 0, 1]) {
      // Force the consumer to expose the dependency metadata selected from the cache.
      const consumer = path.join(lanes[index], 'src-tauri/consumer/src/lib.rs');
      utimesSync(consumer, new Date(), new Date());
      const result = spawnSync(bash, ['-c', `set -eu
        source scripts/lane-check.sh
        ${isolate ? '' : 'configure_lane_workspace_wrapper() { unset RUSTC_WORKSPACE_WRAPPER; };'}
        prepare_dev_frontend_dist() { :; }
        prepare_bundle_stubs() { :; }
        lane_check codeterm-fixture
      `], {
        cwd: lanes[index], encoding: 'utf8', timeout: 120_000,
        env,
      });
      runs.push({ lane: index, status: result.status, log: `${result.stdout}${result.stderr}`, error: result.error?.message });
      if (result.status !== 0) break;
    }
    const deps = path.join(root, 'cache/lanes/debug/deps');
    const metadata = (existsSync(deps) ? readdirSync(deps) : []).filter(name => name.endsWith('.rmeta'));
    return { runs, types: metadata.filter(name => name.startsWith('libfixture_types-')), shared: metadata.filter(name => name.startsWith('libfixture_shared-')) };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

export function verifyLaneMetadata() {
  const broken = reproduceLaneMetadata({ isolate: false });
  assert.equal(broken.runs[0].status, 0, broken.runs[0].log);
  assert.equal(broken.runs[1].status, 101, JSON.stringify(broken));
  assert.match(broken.runs[1].log, /E0425/);
  const fixed = reproduceLaneMetadata();
  assert.equal(fixed.runs.length, 4);
  for (const run of fixed.runs) assert.equal(run.status, 0, `${run.error ?? ''}\n${run.log}`);
  assert.equal(fixed.runs.filter(run => /Checking fixture-shared/.test(run.log)).length, 1, 'registry dependency compiles only once');
  assert.equal(fixed.runs.filter(run => /Checking fixture-types/.test(run.log)).length, 2, 'each lane compiles its dependency once');
  assert.equal(fixed.types.length, 2, 'each workspace retains its own crate metadata');
  assert.equal(fixed.shared.length, 1, 'registry dependencies retain shared metadata');
  return { broken, fixed };
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const result = verifyLaneMetadata();
  console.log(JSON.stringify(result, null, 2));
}
