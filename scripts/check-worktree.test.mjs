// Caller: build.sh script-guards
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import test from 'node:test';
import { prepareCheckWorktree, verifyCheckWorktree } from './check-worktree.mjs';

function fixture() {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'check-worktree-'));
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
      const staging = path.join(f.gateway, 'check-worktree.pending-interrupted');
      const previous = path.join(f.gateway, 'check-worktree.previous-interrupted');
      f.git('worktree', 'add', '--quiet', '--detach', staging, f.sha);
      f.git('worktree', 'move', current, previous);
      fs.renameSync(path.join(previous, 'target'), path.join(staging, 'target'));
      fs.writeFileSync(path.join(f.gateway, 'check-worktree-publication.json'), JSON.stringify({ staging, previous, sha: f.sha }));
      if (phase === 'after-publish') f.git('worktree', 'move', staging, current);
      const ready = await prepareCheckWorktree({ ...f, jobId: 'recovery' });
      verifyCheckWorktree(ready, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
      assert.equal(fs.readFileSync(path.join(ready, 'target', 'warm'), 'utf8'), 'retained');
      assert.equal(fs.existsSync(path.join(f.gateway, 'check-worktree-publication.json')), false);
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
      const previous = path.join(f.gateway, 'check-worktree.previous-leftover');
      if (leftover === 'no-git') {
        fs.mkdirSync(path.join(previous, 'src-tauri'), { recursive: true });
      } else {
        f.git('worktree', 'add', '--quiet', '--detach', previous, f.old);
        fs.rmSync(path.join(f.repository, '.git', 'worktrees', path.basename(previous)), { recursive: true, force: true });
        f.git('worktree', 'prune');
      }
      fs.writeFileSync(path.join(previous, 'src-tauri', 'stale.rs'), 'stale\n');
      const staging = path.join(f.gateway, 'check-worktree.pending-leftover');
      fs.writeFileSync(path.join(f.gateway, 'check-worktree-publication.json'), JSON.stringify({ staging, previous, sha: f.sha }));
      const ready = await prepareCheckWorktree({ ...f, jobId: 'recovery' });
      verifyCheckWorktree(ready, f.sha, f.git('rev-parse', `${f.sha}^{tree}`));
      assert.equal(fs.existsSync(previous), false);
      assert.equal(fs.existsSync(path.join(f.gateway, 'check-worktree-publication.json')), false);
    } finally { fs.rmSync(f.directory, { recursive: true, force: true }); }
  });
}
