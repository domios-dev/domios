import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const script = fileURLToPath(new URL('../../.agents/skills/lane-cleanup/scripts/cleanup-lane.ps1', import.meta.url));

test('orphan lane cleanup unlinks canonical junctions before recursive removal', { skip: process.platform !== 'win32' }, t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'lane-cleanup-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  const canonical = path.join(root, 'canonical');
  const lanes = path.join(root, 'lanes');
  const lane = path.join(lanes, 'orphan');
  const target = path.join(canonical, 'node_modules');
  fs.mkdirSync(target, { recursive: true });
  fs.mkdirSync(lane, { recursive: true });
  const marker = path.join(target, 'package.json');
  fs.writeFileSync(marker, 'canonical proof');
  fs.symlinkSync(target, path.join(lane, 'node_modules'), 'junction');
  fs.writeFileSync(path.join(lane, 'leftover'), 'lane data');
  const alias = path.join(root, 'lane-alias');
  fs.symlinkSync(lanes, alias, 'junction');
  const refused = spawnSync('pwsh', ['-NoProfile', '-File', script, '-LanePath', path.join(alias, 'orphan'),
    '-CanonicalPath', canonical, '-WorktreeRoot', alias, '-AllowUnregisteredResidue'], {
    encoding: 'utf8', timeout: 30000, windowsHide: true,
  });
  assert.notEqual(refused.status, 0);
  assert.match(refused.stderr, /through a reparse point/);
  assert.equal(fs.readFileSync(marker, 'utf8'), 'canonical proof');
  fs.rmdirSync(alias);
  const result = spawnSync('pwsh', ['-NoProfile', '-File', script, '-LanePath', lane,
    '-CanonicalPath', canonical, '-WorktreeRoot', lanes, '-AllowUnregisteredResidue'], {
    encoding: 'utf8', timeout: 30000, windowsHide: true,
  });
  assert.equal(result.status, 0, result.stdout + result.stderr);
  assert.equal(fs.existsSync(lane), false);
  assert.equal(fs.readFileSync(marker, 'utf8'), 'canonical proof');
});
