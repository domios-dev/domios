import fs from 'node:fs';
import path from 'node:path';
import { scanBuildDir, unitPaths } from './rust-build-dir-retention.mjs';

export function orphanLanePaths(root, { exists = fs.existsSync, now = Date.now(), maxAgeDays = 3 } = {}) {
  const inventory = scanBuildDir(root);
  const removed = [];
  const liveNames = new Set();
  for (const unit of inventory.units) {
    const depInfo = path.join(root, unit.profile, 'deps', `${unit.name.replaceAll('-', '_')}-${unit.hash}.d`);
    let source = '';
    try { source = fs.readFileSync(depInfo, 'utf8').replaceAll('\\ ', ' ').replaceAll('\\:', ':').replaceAll('\\', '/'); } catch { /* Missing provenance is retained. */ }
    // Relative dep-info carries no workspace identity, so it cannot authorize orphan collection.
    const lanes = source.split(/\r?\n/).filter(line => line.endsWith(':'))
      .flatMap(line => { const match = /^((?:[A-Za-z]:\/|\/).*?-worktrees\/[^/]+)\//.exec(line); return match ? [match[1]] : []; });
    if (lanes.length && lanes.every(lane => !exists(lane)) && unit.mtime < now - maxAgeDays * 86400000) removed.push(unit);
    else liveNames.add(`${unit.profile}/${unit.name.replaceAll('-', '_')}`);
  }
  const names = new Set(removed.map(unit => `${unit.profile}/${unit.name.replaceAll('-', '_')}`));
  return [...removed.flatMap(unit => unitPaths(root, unit)), ...inventory.sessions
    .filter(session => names.has(`${session.profile}/${session.name}`) && !liveNames.has(`${session.profile}/${session.name}`)
      && session.mtime < now - maxAgeDays * 86400000).map(session => session.dir)];
}
