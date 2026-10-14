// Caller: build.sh check-queue; scripts/check-worktree.test.mjs
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

function git(directory, args) {
  const result = spawnSync('git', ['-C', directory, ...args], { encoding: 'utf8', timeout: 120000, maxBuffer: 64 * 1024 * 1024, windowsHide: true });
  if (result.status !== 0) throw new Error(`check-worktree git ${args[0]} failed (${result.status}): ${result.stderr || result.error?.message}`);
  return result.stdout.trim();
}

export function verifyCheckWorktree(directory, sha, expectedTree) {
  if (git(directory, ['rev-parse', 'HEAD']) !== sha ||
      git(directory, ['rev-parse', 'HEAD^{tree}']) !== expectedTree ||
      git(directory, ['write-tree']) !== expectedTree) {
    throw new Error('check-worktree HEAD/index tree differs from requested source tree; Cargo refused');
  }
  for (const entry of git(directory, ['ls-files', '-v', '-z']).split('\0').filter(Boolean)) {
    if (entry[0] !== 'H' || !fs.existsSync(path.join(directory, entry.slice(2)))) {
      throw new Error(`check-worktree tracked path is missing or hidden by index flags: ${entry.slice(2)}; Cargo refused`);
    }
  }
  git(directory, ['diff-files', '--quiet', '--ignore-submodules=none']);
}

const samePath = (a, b) => {
  const normalize = value => path.resolve(value).toLowerCase();
  return process.platform === 'win32' ? normalize(a) === normalize(b) : path.resolve(a) === path.resolve(b);
};

export function registeredWorktree(repository, directory) {
  return git(repository, ['worktree', 'list', '--porcelain']).split('\n')
    .some(line => line.startsWith('worktree ') && samePath(line.slice('worktree '.length).trim(), directory));
}

const ancestors = file => {
  const out = [];
  for (let directory = path.posix.dirname(file); directory !== '.'; directory = path.posix.dirname(directory)) out.push(directory);
  return out;
};

// Cargo freshness is mtime-based, so a fresh checkout would mark every unchanged crate stale.
export function carryUnchangedMtimes(repository, from, to, sha) {
  let fromSha;
  try {
    fromSha = git(from, ['rev-parse', 'HEAD']);
    git(from, ['update-index', '-q', '--refresh']);
    verifyCheckWorktree(from, fromSha, git(repository, ['rev-parse', `${fromSha}^{tree}`]));
  } catch { return 0; }
  const changed = new Set(git(repository, ['diff', '--no-renames', '--name-only', '-z', fromSha, sha]).split('\0').filter(Boolean));
  const touchedDirectories = new Set([...changed].flatMap(ancestors));
  const directories = new Set();
  let carried = 0;
  const carry = relative => {
    const source = path.join(from, relative);
    if (!fs.existsSync(source)) return;
    const { atime, mtime } = fs.statSync(source);
    fs.utimesSync(path.join(to, relative), atime, mtime);
    carried += 1;
  };
  for (const file of git(to, ['ls-files', '-z']).split('\0').filter(Boolean)) {
    ancestors(file).forEach(directory => directories.add(directory));
    if (!changed.has(file) && fs.lstatSync(path.join(to, file)).isFile()) carry(file);
  }
  [...directories].filter(directory => !touchedDirectories.has(directory))
    .sort((a, b) => b.split('/').length - a.split('/').length)
    .forEach(carry);
  git(to, ['update-index', '-q', '--refresh']);
  return carried;
}

export async function prepareCheckWorktree({ repository, gateway, sha, jobId, beforePublish = async () => {} }) {
  if (!/^[a-zA-Z0-9-]+$/.test(jobId)) throw new Error('Invalid check-worktree job identity');
  gateway = path.resolve(gateway);
  const current = path.join(gateway, 'check-worktree');
  const staging = path.join(gateway, `check-worktree.pending-${jobId}`);
  const previous = path.join(gateway, `check-worktree.previous-${jobId}`);
  const journal = path.join(gateway, 'check-worktree-publication.json');
  const owned = name => {
    const resolved = path.resolve(name);
    if (path.dirname(resolved) !== gateway || !/^check-worktree\.(pending|previous)-[a-zA-Z0-9-]+$/.test(path.basename(resolved))) {
      throw new Error('Refusing check-worktree publication path outside gateway');
    }
    return resolved;
  };
  const move = (from, to) => git(repository, ['worktree', 'move', from, to]);
  const remove = directory => {
    if (fs.existsSync(path.join(directory, '.git')) && registeredWorktree(repository, directory)) {
      git(repository, ['worktree', 'remove', '--force', directory]);
      return;
    }
    // An earlier attempt unregistered it but left files; `worktree remove` would refuse forever.
    fs.rmSync(owned(directory), { recursive: true, force: true });
    git(repository, ['worktree', 'prune']);
  };
  const expectedTree = git(repository, ['rev-parse', `${sha}^{tree}`]);
  if (fs.existsSync(journal)) {
    const interrupted = JSON.parse(fs.readFileSync(journal, 'utf8'));
    const old = owned(interrupted.previous);
    const pending = owned(interrupted.staging);
    if (!fs.existsSync(current) && fs.existsSync(old)) move(old, current);
    if (fs.existsSync(pending)) {
      if (fs.existsSync(path.join(pending, 'target')) && fs.existsSync(current) && !fs.existsSync(path.join(current, 'target'))) {
        fs.renameSync(path.join(pending, 'target'), path.join(current, 'target'));
      }
      remove(pending);
    }
    if (fs.existsSync(old)) {
      if (fs.existsSync(path.join(old, 'target')) && !fs.existsSync(path.join(current, 'target'))) {
        fs.renameSync(path.join(old, 'target'), path.join(current, 'target'));
      }
      remove(old);
    }
    fs.unlinkSync(journal);
  }
  if (fs.existsSync(path.join(current, '.git'))) {
    try { verifyCheckWorktree(current, sha, expectedTree); return current; }
    catch { /* A dirty or interrupted checkout is rebuilt before publication. */ }
  }
  if (fs.existsSync(staging) || fs.existsSync(previous)) throw new Error('check-worktree transaction paths already exist; Cargo refused');
  git(repository, ['worktree', 'add', '--quiet', '--detach', staging, sha]);
  verifyCheckWorktree(staging, sha, expectedTree);
  if (fs.existsSync(path.join(current, '.git'))) carryUnchangedMtimes(repository, current, staging, sha);
  await beforePublish(staging);
  verifyCheckWorktree(staging, sha, expectedTree);
  const temporaryJournal = `${journal}.${jobId}`;
  fs.writeFileSync(temporaryJournal, JSON.stringify({ staging, previous, sha, expectedTree }));
  fs.renameSync(temporaryJournal, journal);
  try {
    if (fs.existsSync(current)) move(current, previous);
    if (fs.existsSync(path.join(previous, 'target'))) {
      if (fs.existsSync(path.join(staging, 'target'))) throw new Error('Staging source unexpectedly owns target; Cargo refused');
      fs.renameSync(path.join(previous, 'target'), path.join(staging, 'target'));
    }
    move(staging, current);
    verifyCheckWorktree(current, sha, expectedTree);
  } catch (error) {
    throw new Error(`check-worktree publication incomplete; Cargo refused, next gateway owner recovers transaction: ${error.message}`);
  }
  if (fs.existsSync(previous)) remove(previous);
  fs.unlinkSync(journal);
  return current;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [repository, gateway, sha, jobId, ownerPid, ownerStart] = process.argv.slice(2);
  try {
    const lock = path.join(gateway, 'lock');
    const read = name => fs.readFileSync(path.join(lock, name), 'utf8').trim();
    if (read('job_id') !== jobId || read('pid') !== ownerPid || read('start_token') !== ownerStart) {
      throw new Error('check-worktree requires the exact live gateway owner; Cargo refused');
    }
    process.stdout.write(`${await prepareCheckWorktree({ repository, gateway, sha, jobId })}\n`);
  } catch (error) {
    console.error(error.message);
    process.exitCode = 75;
  }
}
