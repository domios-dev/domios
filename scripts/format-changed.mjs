import { spawnSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, lstatSync, readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const excludedDirectories = new Set([
  'target', 'node_modules', 'dist', 'build', 'gen', 'references', 'vendor',
  'pty-fork', '.git', '.claude',
]);
const rustExtensions = new Set(['.rs']);
const biomeExtensions = new Set([
  '.ts', '.tsx', '.js', '.jsx', '.mjs', '.cjs', '.mts', '.cts', '.json', '.jsonc',
]);
const BIOME_PATH_BATCH_SIZE = 32;

export function biomeFileBatches(files) {
  const batches = [];
  for (let offset = 0; offset < files.length; offset += BIOME_PATH_BATCH_SIZE) {
    batches.push(files.slice(offset, offset + BIOME_PATH_BATCH_SIZE));
  }
  return batches;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, { encoding: 'utf8', ...options });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    const output = `${result.stdout ?? ''}${result.stderr ?? ''}`.trim();
    throw new Error(`${command} ${args.join(' ')} failed (${result.status})${output ? `\n${output}` : ''}`);
  }
  return result.stdout ?? '';
}

function git(root, args, { allowFailure = false } = {}) {
  const result = spawnSync('git', ['-C', root, ...args], { encoding: 'utf8' });
  if (result.error) throw result.error;
  if (!allowFailure && result.status !== 0) {
    throw new Error(`git ${args.join(' ')} failed (${result.status}): ${result.stderr.trim()}`);
  }
  return { status: result.status, stdout: result.stdout ?? '', stderr: result.stderr ?? '' };
}

function nulPaths(output) {
  return output.split('\0').filter(Boolean);
}

function isEligiblePath(root, relativePath) {
  const normalized = relativePath.replaceAll('\\', '/');
  const segments = normalized.split('/');
  if (segments.some(segment => excludedDirectories.has(segment)) || normalized === 'refs' || normalized.startsWith('refs/')) return false;
  if (segments[0] === 'target-check') {
    const ignored = git(root, ['check-ignore', '-q', '--', normalized], { allowFailure: true }).status === 0;
    const tracked = git(root, ['ls-files', '--', 'target-check'], { allowFailure: true }).stdout.trim().length > 0;
    if (ignored && !tracked) return false;
  }

  const absolutePath = path.resolve(root, ...segments);
  const rootPrefix = `${root}${path.sep}`;
  if (!absolutePath.startsWith(rootPrefix)) return false;
  if (!existsSync(absolutePath) || !lstatSync(absolutePath).isFile()) return false;
  const realPath = realpathSync(absolutePath);
  return realPath === root || realPath.startsWith(rootPrefix);
}

export function collectChangedFiles(root, baseRef = 'origin/develop') {
  const absoluteRoot = realpathSync(root);
  const mergeBase = git(absoluteRoot, ['merge-base', 'HEAD', baseRef]).stdout.trim();
  if (!mergeBase) throw new Error(`Could not find merge-base for ${baseRef}`);

  const committed = nulPaths(git(absoluteRoot, [
    'diff', '--name-only', '-z', '--diff-filter=ACMR', `${mergeBase}..HEAD`,
  ]).stdout);
  const workingTree = nulPaths(git(absoluteRoot, [
    'diff', '--name-only', '-z', '--diff-filter=ACMR', 'HEAD',
  ]).stdout);
  const untracked = nulPaths(git(absoluteRoot, [
    'ls-files', '--others', '--exclude-standard', '-z',
  ]).stdout);

  return [...new Set([...committed, ...workingTree, ...untracked])]
    .filter(relativePath => isEligiblePath(absoluteRoot, relativePath))
    .sort((left, right) => left.localeCompare(right));
}

function packageEditions(root, workspaceManifest) {
  const metadata = JSON.parse(run('cargo', [
    'metadata', '--offline', '--no-deps', '--format-version=1',
    '--manifest-path', workspaceManifest,
  ], { cwd: root }));
  return new Map(metadata.packages.map(pkg => [path.resolve(pkg.manifest_path), pkg.edition]));
}

function nearestCargoManifest(root, filePath) {
  let directory = path.dirname(path.resolve(root, filePath));
  while (directory.startsWith(root)) {
    const manifest = path.join(directory, 'Cargo.toml');
    if (existsSync(manifest)) return manifest;
    if (directory === root) break;
    directory = path.dirname(directory);
  }
  throw new Error(`No Cargo.toml found for ${filePath}`);
}

function biomeCommand() {
  if (process.platform !== 'win32') return ['pnpm', []];
  const corepackCli = path.join(path.dirname(process.execPath), 'node_modules', 'corepack', 'dist', 'pnpm.js');
  if (!existsSync(corepackCli)) throw new Error(`Could not find Corepack pnpm entrypoint: ${corepackCli}`);
  return [process.execPath, [corepackCli]];
}

function editionForFile(root, filePath, editions) {
  let directory = path.dirname(path.resolve(root, filePath));
  while (directory.startsWith(root)) {
    const manifest = path.join(directory, 'Cargo.toml');
    if (editions.has(manifest)) return editions.get(manifest);
    if (directory === root) break;
    directory = path.dirname(directory);
  }
  throw new Error(`No Cargo package edition found for ${filePath}`);
}

function rustfmtConfig(root, filePath) {
  let directory = path.dirname(path.resolve(root, filePath));
  while (directory.startsWith(root)) {
    for (const name of ['rustfmt.toml', '.rustfmt.toml']) {
      const candidate = path.join(directory, name);
      if (existsSync(candidate)) return candidate;
    }
    if (directory === root) break;
    directory = path.dirname(directory);
  }
  return null;
}

function fileFingerprint(filePath) {
  return createHash('sha256').update(readFileSync(filePath)).digest('hex');
}

export function formatChangedFiles(root, baseRef = 'origin/develop') {
  const absoluteRoot = realpathSync(root);
  const changedFiles = collectChangedFiles(absoluteRoot, baseRef);
  const rustFiles = changedFiles.filter(file => rustExtensions.has(path.extname(file)));
  const biomeFiles = changedFiles.filter(file => biomeExtensions.has(path.extname(file)));
  const formatFiles = [...rustFiles, ...biomeFiles];
  const before = new Map(formatFiles.map(file => [file, fileFingerprint(path.join(absoluteRoot, file))]));

  if (rustFiles.length > 0) {
    const editions = new Map();
    const metadataManifests = new Set();
    for (const file of rustFiles) {
      const packageManifest = nearestCargoManifest(absoluteRoot, file);
      const relativeManifest = path.relative(absoluteRoot, packageManifest).replaceAll('\\', '/');
      const workspaceManifest = relativeManifest.startsWith('src-tauri/')
        ? path.join(absoluteRoot, 'src-tauri', 'Cargo.toml')
        : relativeManifest.startsWith('cli/')
          ? path.join(absoluteRoot, 'cli', 'Cargo.toml')
          : packageManifest;
      metadataManifests.add(workspaceManifest);
    }
    for (const workspaceManifest of metadataManifests) {
      for (const [manifest, edition] of packageEditions(absoluteRoot, workspaceManifest)) {
        editions.set(manifest, edition);
      }
    }

    for (const file of rustFiles) {
      const args = ['--edition', editionForFile(absoluteRoot, file, editions), '--config', 'skip_children=true'];
      const config = rustfmtConfig(absoluteRoot, file);
      if (config) args.push('--config-path', config);
      args.push(path.join(absoluteRoot, file));
      run('rustfmt', args, { cwd: absoluteRoot });
    }
  }

  if (biomeFiles.length > 0) {
    const [command, prefixArgs] = biomeCommand();
    for (const batch of biomeFileBatches(biomeFiles)) {
      run(command, [
        ...prefixArgs, 'exec', 'biome', 'check', '--write', '--formatter-enabled=false',
        '--no-errors-on-unmatched', '--', ...batch,
      ], { cwd: absoluteRoot });
    }
  }

  const changedByTools = formatFiles.some(file =>
    before.get(file) !== fileFingerprint(path.join(absoluteRoot, file)));
  console.log(`fmt: rust=${rustFiles.length} biome=${biomeFiles.length} changed=${changedByTools ? 'yes' : 'no'}`);
  return { rustFiles, biomeFiles, changedByTools };
}

function parseArgs(args) {
  let baseRef = 'origin/develop';
  for (let index = 0; index < args.length; index += 1) {
    if (args[index] !== '--base' || !args[index + 1] || args[index + 1].startsWith('--')) {
      throw new Error('Usage: build.sh fmt [--base REF]');
    }
    baseRef = args[index + 1];
    index += 1;
  }
  return baseRef;
}

const invokedPath = process.argv[1] ? path.resolve(process.argv[1]) : '';
if (invokedPath === fileURLToPath(import.meta.url)) {
  try {
    const root = run('git', ['rev-parse', '--show-toplevel'], { cwd: path.dirname(invokedPath) }).trim();
    formatChangedFiles(root, parseArgs(process.argv.slice(2)));
  } catch (error) {
    console.error(`fmt: ${error.message}`);
    process.exitCode = 1;
  }
}
