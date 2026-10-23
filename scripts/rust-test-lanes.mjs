import fs from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { archiveTargets, releaseRustRunners } from './ci-rust-archives.mjs';

const read = name => JSON.parse(fs.readFileSync(new URL(`../tests/${name}`, import.meta.url), 'utf8'));
const runners = releaseRustRunners(read('catalogue.json'), read('catalogue.overlay.json'));
const root = path.resolve(import.meta.dirname, '..').replaceAll('\\', '/');

export function workspaceMembers(repository = root, run = execFileSync) {
  const files = run('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard', '--', '*Cargo.toml', '*Cargo.lock', '*config.toml', '.cargo/config'],
    { cwd: repository, encoding: 'utf8' }).split('\0').filter(Boolean).sort();
  const hash = createHash('sha256').update(repository).update('workspace-members-v1');
  for (const file of files) { hash.update(file); try { hash.update(fs.readFileSync(path.join(repository, file))); } catch { hash.update('missing'); } }
  const receipt = path.join(repository, 'target', 'test-preparation', `workspace-members-${hash.digest('hex')}.json`);
  try {
    const cached = JSON.parse(fs.readFileSync(receipt, 'utf8'));
    if (Array.isArray(cached) && cached.length && cached.every(name => typeof name === 'string' && name.length)) return cached;
  } catch {}
  const metadata = JSON.parse(run('cargo', ['metadata', '--no-deps', '--locked', '--format-version', '1', '--manifest-path', 'src-tauri/Cargo.toml'],
    { cwd: repository, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }));
  const members = new Set(metadata.workspace_members);
  const names = metadata.packages.filter(pkg => members.has(pkg.id)).map(pkg => pkg.name);
  if (!names.length) throw new Error('Cargo returned no workspace members');
  fs.mkdirSync(path.dirname(receipt), { recursive: true });
  fs.writeFileSync(receipt, JSON.stringify(names));
  return names;
}

export function workspaceTestTargets(repository = root, catalogue = runners, members = workspaceMembers(repository)) {
  const selected = archiveTargets('workspace', catalogue);
  const packages = new Set(selected.flatMap((arg, index) => selected[index - 1] === '-p' ? [arg] : []));
  for (const name of members) packages.add(name);
  return ['--no-default-features', ...[...packages].flatMap(name => ['-p', name]), '--tests'];
}

export const FULL_LANES = [
  { lane: 'workspace', manifest: 'src-tauri/Cargo.toml', targets: workspaceTestTargets() },
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
