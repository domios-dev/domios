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

function remove(directory) {
  for (const entry of fs.readdirSync(directory)) {
    const name = path.join(directory, entry);
    const stat = fs.lstatSync(name);
    if (stat.isSymbolicLink()) fs.unlinkSync(name);
    else if (stat.isDirectory()) remove(name);
    else fs.unlinkSync(name);
  }
  fs.rmdirSync(directory);
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
        log(`[lane-target-gc] ${dryRun ? 'would remove' : 'remove'} ${target}: ${stats.bytes} bytes`);
        if (!dryRun) {
          noLinks(target);
          if (inspect(target).newest !== stats.newest) throw new Error(`build output changed: ${target}`);
          remove(target);
          if (fs.existsSync(target)) throw new Error(`build output remains: ${target}`);
        }
        reclaimed += stats.bytes;
      }
    } catch (error) { log(`[lane-target-gc] retained ${lane}: ${error.message}`); }
  }
  return reclaimed;
}
