import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';

export function git(directory, args) {
  const result = spawnSync('git', ['-C', directory, ...args], { encoding: 'utf8', timeout: 120000, maxBuffer: 64 * 1024 * 1024, windowsHide: true });
  if (result.status !== 0) throw new Error(`check-worktree git ${args[0]} failed (${result.status}): ${result.stderr || result.error?.message}`);
  return result.stdout.trim();
}

const normalize = value => process.platform === 'win32' ? path.resolve(value).toLowerCase() : path.resolve(value);

export function registeredWorktree(repository, directory) {
  return git(repository, ['worktree', 'list', '--porcelain', '-z']).split('\0')
    .some(line => line.startsWith('worktree ') && normalize(line.slice(9)) === normalize(directory));
}

export function checkWorktreeRoot(repository, gateway) {
  const common = fs.realpathSync(git(repository, ['rev-parse', '--path-format=absolute', '--git-common-dir']));
  const identity = createHash('sha256').update(normalize(common)).digest('hex');
  return path.join(path.resolve(gateway), 'check-worktrees', identity);
}

export function noLinkedPath(root, directory) {
  const relative = path.relative(root, directory);
  if (relative === '..' || relative.startsWith(`..${path.sep}`) || path.isAbsolute(relative)) {
    throw new Error('Refusing check-worktree path outside gateway');
  }
  for (let cursor = path.resolve(directory); cursor !== root; cursor = path.dirname(cursor)) {
    let stat;
    try { stat = fs.lstatSync(cursor); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
    if (stat?.isSymbolicLink()) throw new Error(`Unsafe check-worktree link: ${cursor}`);
  }
}

export function collectCheckWorktrees(repository, gateway, { dryRun = false, log = console.log } = {}) {
  gateway = fs.realpathSync(gateway);
  const root = checkWorktreeRoot(repository, gateway);
  const collected = [];
  for (const directory of [gateway, root]) {
    noLinkedPath(gateway, directory);
    if (!fs.existsSync(directory)) continue;
    // Recovery owns both sides of an interrupted publication until it commits.
    const journal = path.join(directory, 'check-worktree-publication.json');
    noLinkedPath(gateway, journal);
    if (fs.existsSync(journal)) continue;
    for (const entry of fs.readdirSync(directory)) {
      if (!/^check-worktree\.(broken|pending)-[a-zA-Z0-9-]+$/.test(entry)) continue;
      const candidate = path.join(directory, entry);
      noLinkedPath(gateway, candidate);
      if (!fs.lstatSync(candidate).isDirectory()) continue;
      const registered = registeredWorktree(repository, candidate);
      if (!registered) {
        if (directory !== root) continue;
        if (fs.existsSync(path.join(candidate, '.git'))) continue;
      }
      log(`[check-worktree] ${dryRun ? 'would remove' : 'remove'} ${candidate}`);
      if (!dryRun) {
        if (registered) git(repository, ['worktree', 'remove', '--force', candidate]);
        else fs.rmSync(candidate, { recursive: true, force: true });
      }
      collected.push(candidate);
    }
  }
  return collected;
}
