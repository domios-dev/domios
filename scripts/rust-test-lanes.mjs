import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { archiveTargets, releaseRustRunners } from './ci-rust-archives.mjs';

const read = name => JSON.parse(fs.readFileSync(new URL(`../tests/${name}`, import.meta.url), 'utf8'));
const runners = releaseRustRunners(read('catalogue.json'), read('catalogue.overlay.json'));

export const FULL_LANES = [
  { lane: 'workspace', manifest: 'src-tauri/Cargo.toml', targets: ['--no-default-features', ...archiveTargets('workspace', runners)] },
  { lane: 'cli', manifest: 'cli/Cargo.toml', targets: archiveTargets('cli', runners) },
];

export function testTargetDir(lane, { env = process.env, mode = 'full' } = {}) {
  const base = env.CI_TARGET_ROOT ? path.resolve(env.CI_TARGET_ROOT, 'src-tauri')
    : path.resolve(env.CARGO_TARGET_DIR || 'src-tauri/target');
  const root = mode === 'full' ? base : `${base}-${mode}`;
  return lane.lane === 'cli' ? path.join(root, 'cli') : root;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const lane = FULL_LANES.find(item => item.lane === process.argv[2]);
  if (!lane) throw new Error('Expected workspace or cli');
  process.stdout.write(process.argv[3] === 'target' ? testTargetDir(lane) : lane.targets.join('\n') + '\n');
}
