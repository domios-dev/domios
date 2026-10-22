// Caller: build.sh script-guards
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { prepareCheckWorktree, verifyCheckWorktree } from './check-worktree.mjs';
import { checkWorktreeRoot, collectCheckWorktrees, registeredWorktree } from './check-worktree-paths.mjs';

function fixture() {
  const directory = fs.mkdtempSync(path.join(process.env.CT_BUILD_GATEWAY_FIXTURE_DIR || fs.realpathSync(os.tmpdir()), 'check-worktree-'));
  const repository = path.join(directory, 'repo');
  const gateway = path.join(directory, 'gateway');
  fs.mkdirSync(repository); fs.mkdirSync(gateway);
  const git = (...args) => {
    const result = spawnSync('git', ['-C', repository, ...args], { encoding: 'utf8', windowsHide: true });
    assert.equal(result.status, 0, result.stderr);
    return result.stdout.trim();
  };
  git('init', '--quiet');
  git('config', 'user.name', 'Fixture'); git('config', 'user.email', 'fixture@example.invalid');
  git('config', 'core.autocrlf', 'false');
  fs.mkdirSync(path.join(repository, 'src-tauri', 'src'), { recursive: true });
  fs.mkdirSync(path.join(repository, 'cli'));
  fs.writeFileSync(path.join(repository, '.gitignore'), 'target/\n');
  fs.writeFileSync(path.join(repository, 'src-tauri', 'src', 'lib.rs'), 'mod owned;\n');
  fs.writeFileSync(path.join(repository, 'src-tauri', 'src', 'owned.rs'), 'pub fn owned() {}\n');
  fs.writeFileSync(path.join(repository, 'cli', 'Cargo.toml'), '[package]\nname="fixture"\n');
  git('add', '-A'); git('commit', '--quiet', '-m', 'fixture old tree');
  const old = git('rev-parse', 'HEAD');
  fs.mkdirSync(path.join(repository, 'src-tauri', 'crates', 'core'), { recursive: true });
  git('mv', 'src-tauri/src', 'src-tauri/crates/core/src');
  git('commit', '--quiet', '-m', 'fixture moved tree');
  const sha = git('rev-parse', 'HEAD');
  return { directory, repository, gateway, git, old, sha };
}
function deferred() { let resolve; const promise = new Promise(r => resolve = r); return { promise, resolve }; }

test('two independent clones publish different SHAs without touching the other registration', async () => {
  const f = fixture();
  try {
    const second = path.join(f.directory, 'second');
    f.git('clone', '--quiet', f.repository, second);
    const first = await prepareCheckWorktree({ ...f, sha: f.old, jobId: 'first' });
    const before = f.git('worktree', 'list', '--porcelain');
    const next = await prepareCheckWorktree({ ...f, repository: second, jobId: 'second' });
    assert.notEqual(first, next);
    verifyCheckWorktree(first, f.old, f.git('rev-parse', `${f.old}^{tree}`));
    verifyCheckWorktree(next, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
    assert.equal(f.git('worktree', 'list', '--porcelain'), before);
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});

for (const corruption of ['missing-module', 'missing-manifest', 'wrong-index', 'skip-worktree-missing-module']) {
  test(`check-worktree repairs ${corruption} before allowing compilation`, async () => {
    const f = fixture();
    try {
      const current = await prepareCheckWorktree({ ...f, jobId: 'initial' });
      const cache = path.join(current, 'target', 'app', 'warm');
      fs.mkdirSync(path.dirname(cache), { recursive: true }); fs.writeFileSync(cache, 'retained');
      if (corruption === 'missing-module') fs.unlinkSync(path.join(current, 'src-tauri/crates/core/src/owned.rs'));
      if (corruption === 'missing-manifest') fs.unlinkSync(path.join(current, 'cli/Cargo.toml'));
      if (corruption === 'skip-worktree-missing-module') {
        const result = spawnSync('git', ['-C', current, 'update-index', '--skip-worktree', 'src-tauri/crates/core/src/owned.rs'], { encoding: 'utf8' });
        assert.equal(result.status, 0, result.stderr);
        fs.unlinkSync(path.join(current, 'src-tauri/crates/core/src/owned.rs'));
      }
      if (corruption === 'wrong-index') {
        const result = spawnSync('git', ['-C', current, 'read-tree', f.old], { encoding: 'utf8' });
        assert.equal(result.status, 0, result.stderr);
      }
      assert.throws(() => verifyCheckWorktree(current, f.sha, f.git('rev-parse', `${f.sha}^{tree}`)));
      const repaired = await prepareCheckWorktree({ ...f, jobId: 'repair' });
      verifyCheckWorktree(repaired, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
      assert.equal(fs.readFileSync(cache, 'utf8'), 'retained');
      assert.ok(fs.existsSync(path.join(repaired, 'cli/Cargo.toml')));
      assert.ok(fs.existsSync(path.join(repaired, 'src-tauri/crates/core/src/owned.rs')));
    } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
  });
}

test('a rename wave stays unpublished until the staging tree is verified and the event releases it', async () => {
  const f = fixture();
  const staged = deferred(), permit = deferred();
  try {
    const current = await prepareCheckWorktree({ ...f, sha: f.old, jobId: 'old' });
    let compilation = false;
    const switched = prepareCheckWorktree({ ...f, jobId: 'rename', beforePublish: async directory => {
      staged.resolve(directory); await permit.promise;
    } }).then(directory => { compilation = true; return directory; });
    await staged.promise;
    assert.equal(compilation, false);
    assert.ok(fs.existsSync(path.join(current, 'src-tauri/src/owned.rs')));
    assert.equal(fs.existsSync(path.join(current, 'src-tauri/crates/core/src/owned.rs')), false);
    permit.resolve();
    const ready = await switched;
    assert.equal(compilation, true);
    assert.equal(fs.existsSync(path.join(ready, 'src-tauri/src/owned.rs')), false);
    verifyCheckWorktree(ready, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
  } finally { permit.resolve(); fs.rmSync(f.directory, { recursive: true, force: true }); }
});

test('staging corruption at the publication barrier preserves the old checkout and never compiles', async () => {
  const f = fixture();
  try {
    const current = await prepareCheckWorktree({ ...f, sha: f.old, jobId: 'old' });
    let compilation = false;
    await assert.rejects(prepareCheckWorktree({ ...f, jobId: 'corrupt', beforePublish: async directory => {
      fs.unlinkSync(path.join(directory, 'cli/Cargo.toml'));
    } }).then(() => compilation = true));
    assert.equal(compilation, false);
    verifyCheckWorktree(current, f.old, f.git('rev-parse', `${f.old}^{tree}`));
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});
for (const phase of ['before-publish', 'after-publish']) {
  test(`an interrupted ${phase} transaction recovers the verified tree and warm cache`, async () => {
    const f = fixture();
    try {
      const current = await prepareCheckWorktree({ ...f, sha: f.old, jobId: 'old' });
      fs.mkdirSync(path.join(current, 'target'));
      fs.writeFileSync(path.join(current, 'target', 'warm'), 'retained');
      const staging = path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree.pending-interrupted');
      const previous = path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree.previous-interrupted');
      f.git('worktree', 'add', '--quiet', '--detach', staging, f.sha);
      f.git('worktree', 'move', current, previous);
      fs.renameSync(path.join(previous, 'target'), path.join(staging, 'target'));
      fs.writeFileSync(path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree-publication.json'), JSON.stringify({ staging, previous, sha: f.sha }));
      if (phase === 'after-publish') f.git('worktree', 'move', staging, current);
      const ready = await prepareCheckWorktree({ ...f, jobId: 'recovery' });
      verifyCheckWorktree(ready, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
      assert.equal(fs.readFileSync(path.join(ready, 'target', 'warm'), 'utf8'), 'retained');
      assert.equal(fs.existsSync(path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree-publication.json')), false);
      assert.equal(fs.existsSync(previous), false);
    } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
  });
}

test('a new SHA keeps the mtimes of unchanged files so Cargo stays fresh, and changed files stay new', async () => {
  const f = fixture();
  try {
    const current = await prepareCheckWorktree({ ...f, sha: f.old, jobId: 'old' });
    const settled = new Date('2020-01-01T00:00:00Z');
    for (const file of ['cli/Cargo.toml', 'cli', 'src-tauri/src/owned.rs']) fs.utimesSync(path.join(current, file), settled, settled);
    const ready = await prepareCheckWorktree({ ...f, jobId: 'new' });
    verifyCheckWorktree(ready, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
    assert.equal(fs.statSync(path.join(ready, 'cli/Cargo.toml')).mtimeMs, settled.getTime());
    assert.equal(fs.statSync(path.join(ready, 'cli')).mtimeMs, settled.getTime());
    assert.ok(fs.statSync(path.join(ready, 'src-tauri/crates/core/src/owned.rs')).mtimeMs > settled.getTime());
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});

test('a dirty previous checkout carries no mtimes', async () => {
  const f = fixture();
  try {
    const current = await prepareCheckWorktree({ ...f, sha: f.old, jobId: 'old' });
    const settled = new Date('2020-01-01T00:00:00Z');
    fs.writeFileSync(path.join(current, 'cli/Cargo.toml'), '[package]\nname="edited"\n');
    fs.utimesSync(path.join(current, 'cli/Cargo.toml'), settled, settled);
    const ready = await prepareCheckWorktree({ ...f, jobId: 'new' });
    assert.ok(fs.statSync(path.join(ready, 'cli/Cargo.toml')).mtimeMs > settled.getTime());
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});

for (const leftover of ['no-git', 'unregistered-git']) {
  test(`recovery removes a ${leftover} previous leftover instead of refusing every later job`, async () => {
    const f = fixture();
    try {
      await prepareCheckWorktree({ ...f, sha: f.old, jobId: 'old' });
      const previous = path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree.previous-leftover');
      if (leftover === 'no-git') {
        fs.mkdirSync(path.join(previous, 'src-tauri'), { recursive: true });
      } else {
        f.git('worktree', 'add', '--quiet', '--detach', previous, f.old);
        fs.rmSync(path.join(f.repository, '.git', 'worktrees', path.basename(previous)), { recursive: true, force: true });
        f.git('worktree', 'prune');
      }
      fs.writeFileSync(path.join(previous, 'src-tauri', 'stale.rs'), 'stale\n');
      const staging = path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree.pending-leftover');
      fs.writeFileSync(path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree-publication.json'), JSON.stringify({ staging, previous, sha: f.sha }));
      const ready = await prepareCheckWorktree({ ...f, jobId: 'recovery' });
      verifyCheckWorktree(ready, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
      assert.equal(fs.existsSync(previous), false);
      assert.equal(fs.existsSync(path.join(checkWorktreeRoot(f.repository, f.gateway), 'check-worktree-publication.json')), false);
    } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
  });
}

test('linked lanes share their common repository cache and preserve a legacy checkout', async () => {
  const f = fixture();
  try {
    const legacy = path.join(f.gateway, 'check-worktree');
    f.git('worktree', 'add', '--quiet', '--detach', legacy, f.old);
    const lane = path.join(f.directory, 'lane');
    f.git('worktree', 'add', '--quiet', '--detach', lane, f.sha);
    const before = f.git('worktree', 'list', '--porcelain');
    assert.equal(checkWorktreeRoot(lane, f.gateway), checkWorktreeRoot(f.repository, f.gateway));
    const ready = await prepareCheckWorktree({ ...f, repository: lane, jobId: 'lane' });
    assert.notEqual(ready, legacy);
    verifyCheckWorktree(legacy, f.old, f.git('rev-parse', `${f.old}^{tree}`));
    const after = f.git('worktree', 'list', '--porcelain');
    for (const registration of before.split('\n\n').filter(Boolean)) assert.ok(after.includes(registration));
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});

test('gc removes only owned broken/pending leftovers and preserves active and foreign trees', async () => {
  const f = fixture();
  try {
    const current = await prepareCheckWorktree({ ...f, jobId: 'live' });
    const root = path.dirname(current);
    const second = path.join(f.directory, 'second');
    f.git('clone', '--quiet', f.repository, second);
    const foreign = path.join(f.gateway, 'check-worktree.pending-foreign');
    const result = spawnSync('git', ['-C', second, 'worktree', 'add', '--quiet', '--detach', foreign, f.sha], { encoding: 'utf8' });
    assert.equal(result.status, 0, result.stderr);
    const legacy = path.join(f.gateway, 'check-worktree');
    f.git('worktree', 'add', '--quiet', '--detach', legacy, f.old);
    const targets = [path.join(f.gateway, 'check-worktree.broken-20261015T140920'),
      path.join(root, 'check-worktree.pending-owned')];
    for (const target of targets) f.git('worktree', 'add', '--quiet', '--detach', target, f.sha);
    const orphan = path.join(root, 'check-worktree.broken-unregistered');
    fs.mkdirSync(orphan);
    fs.writeFileSync(path.join(orphan, 'leftover'), 'owned');
    const unknown = path.join(f.gateway, 'check-worktree.pending-unknown');
    fs.mkdirSync(unknown);
    targets.push(orphan);
    const quiet = { log: () => {} };
    assert.deepEqual(collectCheckWorktrees(f.repository, f.gateway, { ...quiet, dryRun: true }).sort(), targets.sort());
    for (const target of targets) assert.ok(fs.existsSync(target));
    assert.deepEqual(collectCheckWorktrees(f.repository, f.gateway, quiet).sort(), targets);
    for (const target of targets) {
      assert.equal(fs.existsSync(target), false);
      assert.equal(registeredWorktree(f.repository, target), false);
    }
    for (const target of [current, legacy, foreign, unknown]) assert.ok(fs.existsSync(target));
    verifyCheckWorktree(current, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
    assert.equal(registeredWorktree(second, foreign), true);
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});

test('a gateway through a linked ancestor prepares and collects while in-tree links are refused', async () => {
  const f = fixture();
  try {
    const home = path.join(f.directory, 'home');
    const alias = path.join(f.directory, 'linked-home');
    const physicalGateway = path.join(home, 'gateway');
    fs.mkdirSync(home);
    fs.renameSync(f.gateway, physicalGateway);
    const linkType = process.platform === 'win32' ? 'junction' : 'dir';
    fs.symlinkSync(home, alias, linkType);
    const gateway = path.join(alias, 'gateway');
    const current = await prepareCheckWorktree({ ...f, gateway, jobId: 'live' });
    assert.equal(current, path.join(checkWorktreeRoot(f.repository, fs.realpathSync(physicalGateway)), 'check-worktree'));
    verifyCheckWorktree(current, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
    const pending = path.join(path.dirname(current), 'check-worktree.pending-stale');
    f.git('worktree', 'add', '--quiet', '--detach', pending, f.sha);
    assert.deepEqual(collectCheckWorktrees(f.repository, gateway, { log: () => {} }), [pending]);
    assert.equal(fs.existsSync(pending), false);
    assert.equal(registeredWorktree(f.repository, pending), false);
    assert.equal(registeredWorktree(f.repository, current), true);

    const namespace = path.join(physicalGateway, 'check-worktrees');
    const saved = path.join(f.directory, 'saved-namespace');
    fs.renameSync(namespace, saved);
    fs.symlinkSync(saved, namespace, linkType);
    await assert.rejects(prepareCheckWorktree({ ...f, gateway, jobId: 'linked' }), /Unsafe check-worktree link/);
    assert.throws(() => collectCheckWorktrees(f.repository, gateway), /Unsafe check-worktree link/);
    fs.unlinkSync(namespace);
    fs.renameSync(saved, namespace);

    for (const entry of ['check-worktree.pending-linked', 'check-worktree.previous-linked',
      'check-worktree-publication.json', 'check-worktree-publication.json.linked']) {
      const linked = path.join(path.dirname(current), entry);
      fs.symlinkSync(path.join(f.directory, 'missing'), linked, linkType);
      await assert.rejects(prepareCheckWorktree({ ...f, gateway, jobId: 'linked' }), /Unsafe check-worktree link/);
      if (entry === 'check-worktree.pending-linked' || entry === 'check-worktree-publication.json') {
        assert.throws(() => collectCheckWorktrees(f.repository, gateway), /Unsafe check-worktree link/);
      }
      fs.unlinkSync(linked);
    }
    verifyCheckWorktree(current, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});

test('gc preserves interrupted publications and refuses linked candidate directories', async () => {
  const f = fixture();
  try {
    const current = await prepareCheckWorktree({ ...f, jobId: 'live' });
    const root = path.dirname(current);
    const pending = path.join(root, 'check-worktree.pending-interrupted');
    f.git('worktree', 'add', '--quiet', '--detach', pending, f.sha);
    const journal = path.join(root, 'check-worktree-publication.json');
    fs.writeFileSync(journal, JSON.stringify({ staging: pending }));
    assert.deepEqual(collectCheckWorktrees(f.repository, f.gateway), []);
    assert.ok(fs.existsSync(pending));
    fs.unlinkSync(journal);
    const link = path.join(f.gateway, 'check-worktree.broken-linked');
    fs.symlinkSync(root, link, process.platform === 'win32' ? 'junction' : 'dir');
    assert.throws(() => collectCheckWorktrees(f.repository, f.gateway), /Unsafe check-worktree link/);
    assert.ok(fs.existsSync(current));
    assert.ok(fs.existsSync(pending));
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});

test('FIXTURE extracted check-queue lane and gc entries succeed for two clones with fake Cargo', { timeout: 60000 }, async () => {
  const f = fixture();
  const sourceRoot = path.resolve(import.meta.dirname, '..');
  const source = fs.readFileSync(path.join(sourceRoot, 'build.sh'), 'utf8');
  const functions = ['check_queue_targets', 'check_queue_errors', 'check_queue_warnings',
    'configure_product_cargo', 'enter_check_worktree', 'run_check_queue_cargo', 'check_queue_run']
    .map(name => source.match(new RegExp(`^${name}\\(\\) \\{[\\s\\S]*?^\\}`, 'm'))[0]).join('\n');
  const gc = source.match(/^  gc\)\n([\s\S]*?)^    ;;/m)[1];
  const bash = process.platform === 'win32' ? 'C:/Program Files/Git/bin/bash.exe' : 'bash';
  try {
    f.git('branch', 'develop', f.old);
    const second = path.join(f.directory, 'second');
    f.git('clone', '--quiet', f.repository, second);
    const secondGit = (...args) => {
      const result = spawnSync('git', ['-C', second, ...args], { encoding: 'utf8' });
      assert.equal(result.status, 0, result.stderr);
      return result.stdout.trim();
    };
    secondGit('branch', 'develop', f.old);
    fs.appendFileSync(path.join(second, 'src-tauri/crates/core/src/owned.rs'), 'pub fn second() {}\n');
    secondGit('add', '-A');
    secondGit('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--quiet', '-m', 'fixture second clone');
    const home = path.join(f.directory, 'home');
    fs.mkdirSync(home);
    const calls = path.join(f.directory, 'cargo.jsonl');
    const body = `
      set -eo pipefail
      source "$1/scripts/build-gateway.sh"
      source "$1/scripts/cache-policy.sh"
      fixture_repository="$2"
      ct_gateway_set_fixture_dir "$3"
      export HOME="$4" FIXTURE_CARGO_CALLS="$5"
      export CODETERM_BUILD_GATEWAY_MODE=fail-fast
      unset CT_BUILD_JOB_ID
      CMD=check-queue
      ${functions}
      enter_cargo_gateway() { ct_gateway_acquire "$*"; }
      enter_build_gateway() { ct_gateway_acquire "$*"; }
      enter_stable_build_path() { :; }
      prepare_dev_frontend_dist() { :; }
      prepare_bundle_stubs() { :; }
      configure_dev_cargo() { CT_DEV_CARGO_FEATURES=embeddings; }
      check_queue_worktree() {
        node "$CT_GATEWAY_SCRIPT_ROOT/scripts/check-worktree.mjs" "$fixture_repository" "$(ct_gateway_dir)" "$1" "$CT_BUILD_JOB_ID" "$CT_GATEWAY_OWNER_PID" "$CT_GATEWAY_PROCESS_START"
      }
      cargo() {
        node -e '
          const fs = require("node:fs"), path = require("node:path");
          const args = process.argv.slice(1);
          if (args[0] === "metadata") console.log(JSON.stringify({workspace_members:["core"], packages:[{id:"core",name:"core",manifest_path:path.join(process.cwd(),"src-tauri/crates/core/Cargo.toml"),targets:[{kind:["lib"]}]}]}));
          else {
            if (args[0] !== "clippy") process.exit(91);
            fs.appendFileSync(process.env.FIXTURE_CARGO_CALLS, JSON.stringify({cwd:process.cwd(),args})+"\\n");
          }
        ' "$@"
      }
      cd "$fixture_repository"
      echo "FIXTURE extracted check_queue_run --lane clone=$fixture_repository"
      check_queue_run --lane
      echo "FIXTURE ready=$CT_CHECK_WORKTREE"
      ct_gateway_finish 0
      cd "$fixture_repository"
      CMD=gc
      unset CT_BUILD_JOB_ID
      set -- gc
      ${gc}
      echo 'FIXTURE extracted build.sh gc exit=0'
    `;
    const entry = path.join(f.directory, 'build-entries.sh');
    fs.writeFileSync(entry, body);
    let firstRegistrations;
    const roots = [];
    for (const repository of [f.repository, second]) {
      fs.mkdirSync(path.join(repository, 'scripts'), { recursive: true });
      fs.copyFileSync(path.join(sourceRoot, 'scripts/clippy-allow.sh'), path.join(repository, 'scripts/clippy-allow.sh'));
      const root = checkWorktreeRoot(repository, f.gateway);
      fs.mkdirSync(root, { recursive: true });
      const leftover = path.join(root, 'check-worktree.pending-stale');
      fs.mkdirSync(leftover);
      const result = spawnSync(bash, [entry, sourceRoot, repository, f.gateway, home, calls],
        { cwd: repository, encoding: 'utf8', timeout: 25000 });
      console.log(result.stdout);
      if (result.stderr) console.error(result.stderr);
      assert.equal(result.status, 0, `${result.error || ''}${result.stderr}${result.stdout}`);
      assert.match(result.stdout, /check-queue: .*lane=1 .*exit=0/);
      assert.match(result.stdout, /FIXTURE extracted build.sh gc exit=0/);
      assert.equal(fs.existsSync(leftover), false);
      roots.push(path.join(root, 'check-worktree'));
      if (repository === f.repository) firstRegistrations = f.git('worktree', 'list', '--porcelain');
    }
    assert.notEqual(roots[0], roots[1]);
    assert.equal(f.git('worktree', 'list', '--porcelain'), firstRegistrations);
    const recorded = fs.readFileSync(calls, 'utf8').trim().split('\n').map(JSON.parse);
    assert.equal(recorded.length, 2);
    assert.deepEqual(recorded.map(call => call.cwd), roots);
    for (const call of recorded) assert.ok(call.args.includes('--lib'));
    const jobs = fs.readdirSync(path.join(f.gateway, 'jobs')).filter(file => file.endsWith('.json'))
      .map(file => JSON.parse(fs.readFileSync(path.join(f.gateway, 'jobs', file), 'utf8')));
    assert.equal(jobs.length, 4);
    for (const job of jobs) {
      assert.equal(job.state, 'completed');
      assert.equal(job.exit_code, 0);
    }
    assert.equal(fs.existsSync(path.join(f.gateway, 'lock')), false);
  } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
});
