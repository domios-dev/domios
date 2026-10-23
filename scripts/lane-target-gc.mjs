// Caller: scripts/cache-retention.mjs
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';

function noLinks(directory) {
  for (let cursor = path.resolve(directory); ; cursor = path.dirname(cursor)) {
    if (fs.lstatSync(cursor).isSymbolicLink()) throw new Error(`linked build path: ${cursor}`);
    if (path.dirname(cursor) === cursor) break;
  }
}

function inspect(directory) {
  let newest = 0;
  let bytes = 0;
  const pending = [directory];
  while (pending.length) {
    const name = pending.pop();
    const stat = fs.lstatSync(name);
    newest = Math.max(newest, stat.mtimeMs);
    if (stat.isSymbolicLink()) continue;
    if (stat.isDirectory()) {
      for (const child of fs.readdirSync(name)) pending.push(path.join(name, child));
    }
    else bytes += stat.size;
  }
  return { newest, bytes };
}

export function trimLaneTargets(canonical, { checkout = canonical, now = Date.now(), maxAgeDays = 3,
  dryRun = false, runner = spawnSync, log = console.log } = {}) {
  const root = path.join(path.dirname(canonical), `${path.basename(canonical)}-worktrees`);
  if (!fs.existsSync(root)) return 0;
  noLinks(root);
  let reclaimed = 0;
  for (const name of fs.readdirSync(root)) {
    const lane = path.join(root, name);
    if (path.resolve(lane) === path.resolve(checkout)) continue;
    try {
      noLinks(lane);
      if (!fs.statSync(lane).isDirectory()) continue;
      for (const relative of ['target', 'src-tauri/target', 'cli/target']) {
        const target = path.join(lane, relative);
        if (!fs.existsSync(target)) continue;
        noLinks(target);
        const tracked = runner('git', ['ls-files', '-z', '--', relative], { cwd: lane, encoding: 'utf8' });
        if (tracked.status !== 0 || tracked.stdout.length) {
          log(`[lane-target-gc] retained ${target}: tracked content or unavailable index`);
          continue;
        }
        const stats = inspect(target);
        if (stats.newest >= now - maxAgeDays * 86400000) continue;
        noLinks(target);
        if (inspect(target).newest !== stats.newest) throw new Error(`build output changed: ${target}`);
        const python = ['python3', 'python'].find(program => runner(program, ['--version'], { encoding: 'utf8' }).status === 0);
        if (!python) throw new Error('Python is required for Cargo locks');
        const result = runner(python, [path.join(import.meta.dirname, 'cargo-build-gc.py'), '--tree', path.dirname(target), target,
          ...(dryRun ? ['--dry-run'] : [])], { encoding: 'utf8' });
        if (result.status !== 0) throw new Error(result.stderr || result.error?.message);
        const outcome = JSON.parse(result.stdout);
        if (outcome.status === 'protected') { log(`[lane-target-gc] retained ${target}: ${outcome.reason}`); continue; }
        log(`[lane-target-gc] ${outcome.status} ${target}: ${outcome.bytes} bytes`);
        reclaimed += outcome.bytes;
      }
    } catch (error) { log(`[lane-target-gc] retained ${lane}: ${error.message}`); }
  }
  return reclaimed;
}
