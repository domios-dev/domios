import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { FULL_LANES, testTargetDir } from './rust-test-lanes.mjs';

export function treeStats(directory) {
  const stat = fs.lstatSync(directory);
  if (stat.isSymbolicLink()) throw new Error(`linked cache path: ${directory}`);
  if (!stat.isDirectory()) return { bytes: stat.size, newest: stat.mtimeMs };
  return fs.readdirSync(directory).reduce((total, name) => {
    const child = treeStats(path.join(directory, name));
    return { bytes: total.bytes + child.bytes, newest: Math.max(total.newest, child.newest) };
  }, { bytes: 0, newest: stat.mtimeMs });
}

export function gatewayCandidates(gateway, { env = process.env } = {}) {
  const protectedPaths = new Set();
  for (const file of ['check-worktree-publication.json']) {
    try {
      const record = JSON.parse(fs.readFileSync(path.join(gateway, file), 'utf8'));
      for (const value of Object.values(record)) if (typeof value === 'string' && path.isAbsolute(value)) protectedPaths.add(path.resolve(value));
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
  }
  for (const file of ['lock/worktree']) {
    try { protectedPaths.add(path.resolve(fs.readFileSync(path.join(gateway, file), 'utf8').trim())); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  let activeJob;
  try { activeJob = fs.readFileSync(path.join(gateway, 'lock/job_id'), 'utf8').trim(); }
  catch (error) { if (error.code !== 'ENOENT') throw error; }
  const candidates = fs.readdirSync(gateway).filter(name => /^check-worktree\.(broken|pending|previous)-[A-Za-z0-9-]+$/.test(name))
    .filter(name => !activeJob || !name.endsWith(`-${activeJob}`))
    .map(name => ({ root: gateway, dir: path.join(gateway, name) }));
  if (env.CI_TARGET_ROOT && fs.existsSync(env.CI_TARGET_ROOT)) {
    for (const lane of FULL_LANES) for (const mode of ['full', 'platform', 'embeddings']) protectedPaths.add(testTargetDir(lane, { env, mode }));
    const root = path.resolve(env.CI_TARGET_ROOT);
    for (const name of fs.readdirSync(root).filter(name => /^(src-tauri|cli)(-(platform|embeddings))?$/.test(name))) candidates.push({ root, dir: path.join(root, name) });
  }
  return candidates.filter(({ dir }) => !protectedPaths.has(path.resolve(dir)));
}

export function trimGatewayCaches(gateway, { dryRun = false, now = Date.now(), maxAgeDays = 3,
  env = process.env, runner = spawnSync, log = console.log } = {}) {
  let reclaimed = 0;
  const helper = path.join(import.meta.dirname, 'cargo-build-gc.py');
  for (const { root, dir } of gatewayCandidates(gateway, { env })) {
    try {
      const stats = treeStats(dir);
      if (stats.newest >= now - maxAgeDays * 86400000) continue;
      for (let cursor = path.resolve(dir); ; cursor = path.dirname(cursor)) {
        if (fs.lstatSync(cursor).isSymbolicLink()) throw new Error(`linked cache parent: ${cursor}`);
        if (path.dirname(cursor) === cursor) break;
      }
      const python = ['python3', 'python'].find(program => runner(program, ['--version'], { encoding: 'utf8' }).status === 0);
      if (!python) throw new Error('Python is required for Cargo locks');
      const result = runner(python, [helper, '--tree', root, dir, ...(dryRun ? ['--dry-run'] : [])], { encoding: 'utf8' });
      if (result.status !== 0) throw new Error(result.stderr || result.error?.message);
      const outcome = JSON.parse(result.stdout);
      log(`[cache-policy] ${outcome.status} ${outcome.bytes} bytes: ${dir}${outcome.reason ? ` (${outcome.reason})` : ''}`);
      reclaimed += outcome.bytes;
    } catch (error) { log(`[cache-policy] retained ${dir}: ${error.message}`); }
  }
  return reclaimed;
}
