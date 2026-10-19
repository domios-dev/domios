// Caller: github-release/scripts/build-mac.sh, release-macos-x86_64.sh
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { lstat, mkdir, mkdtemp, readFile, readdir, rm, symlink } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateManifest } from './package-server-component-manifest.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');

async function regularBytes(file) {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink() || !stat.size) throw new Error('expected regular package file: ' + file);
  return readFile(file);
}

export async function verifyMacPayload({ app, stage, expected }) {
  const names = (await readdir(path.join(app, 'Contents', 'MacOS'))).sort();
  if (JSON.stringify(names) !== JSON.stringify(['domios', 'domios-server'])) {
    throw new Error('unexpected macOS executable inventory: ' + names.join(', '));
  }
  for (const [relative, bytes] of [
    ['Contents/MacOS/domios-server', expected.server],
    ['Contents/Resources/cli/codeterm', expected.cli],
    ['Contents/Resources/server-component.json', expected.manifest],
  ]) {
    if (!(await regularBytes(path.join(app, relative))).equals(bytes)) {
      throw new Error('macOS bundling changed frozen bytes: ' + relative + ' (stage ' + stage + ')');
    }
  }
  await regularBytes(path.join(app, 'Contents', 'MacOS', 'domios'));
}

function command(program, args, cwd) {
  execFileSync(program, args, { cwd, stdio: 'inherit', timeout: 240000 });
}

export async function bundleMacPair({ repo = root, target, env = process.env,
  run = command, linkApplications = destination => symlink('/Applications', destination, 'dir'),
  sourceSha = execFileSync('git', ['rev-parse', 'HEAD'], { cwd: repo, encoding: 'utf8' }).trim() }) {
  if (!['aarch64-apple-darwin', 'x86_64-apple-darwin'].includes(target)) throw new Error('unsupported macOS bundle target');
  const identity = env.APPLE_SIGNING_IDENTITY;
  const keychain = env.RELEASE_KEYCHAIN;
  if (!identity || !keychain || !/^[A-Z0-9]{10}$/.test(env.APPLE_TEAM_ID ?? '')) {
    throw new Error('approved macOS signing identity, team and keychain are required');
  }
  const version = JSON.parse(await readFile(path.join(repo, 'package.json'), 'utf8')).version;
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error('invalid macOS package version');
  const targetArgs = target === 'x86_64-apple-darwin' ? ['--target', target] : [];
  const release = path.join(repo, 'src-tauri', 'target', ...targetArgs.slice(1), 'release');
  const bundleRoot = path.join(release, 'bundle');
  const app = path.join(bundleRoot, 'macos', 'CodeTerm.app');
  const stage = path.join(repo, 'target', 'domios-package');
  const expected = {
    server: await regularBytes(path.join(stage, 'domios-server', 'domios-server')),
    cli: await regularBytes(path.join(stage, 'cli', 'codeterm')),
    manifest: await regularBytes(path.join(stage, 'server-component.json')),
  };
  const manifest = validateManifest(JSON.parse(expected.manifest.toString('utf8')));
  if (manifest.server_build !== sourceSha || manifest.client_identity.kind !== 'macos_designated_requirement' ||
      manifest.client_identity.team_id !== env.APPLE_TEAM_ID ||
      manifest.server_executable !== '../MacOS/domios-server' ||
      manifest.server_sha256 !== hash(expected.server)) throw new Error('macOS staged component identity differs from final signed server');
  const invoke = async (program, args) => run(program, args, repo);
  await invoke('pnpm', ['tauri', 'bundle', '--no-sign', '--no-binary-patching', ...targetArgs,
    '--features', env.CODETERM_RELEASE_FEATURES || 'embeddings', '--bundles', 'app',
    '--config', '{"bundle":{"createUpdaterArtifacts":false}}']);
  await verifyMacPayload({ app, stage, expected });
  // Signing the outer app without --deep preserves the pre-signed CLI/server bytes.
  await invoke('codesign', ['--force', '--options', 'runtime', '--timestamp', '--keychain', keychain,
    '--entitlements', path.join(repo, 'src-tauri', 'Entitlements.plist'), '--sign', identity, app]);
  await verifyMacPayload({ app, stage, expected });
  await invoke('codesign', ['--verify', '--strict', '--deep',
    '--test-requirement=' + manifest.client_identity.requirement, app]);
  const arch = target.startsWith('aarch64') ? 'aarch64' : 'x64';
  const dmg = path.join(bundleRoot, 'dmg', `CodeTerm_${version}_${arch}.dmg`);
  await mkdir(path.dirname(dmg), { recursive: true });
  const scratch = await mkdtemp(path.join(bundleRoot, '.pair-dmg-'));
  let mounted = false;
  const mountpoint = path.join(scratch, 'readback');
  try {
    const imageSource = path.join(scratch, 'source');
    await mkdir(imageSource);
    await invoke('ditto', ['--rsrc', '--extattr', app, path.join(imageSource, 'CodeTerm.app')]);
    await linkApplications(path.join(imageSource, 'Applications'));
    await invoke('hdiutil', ['create', '-volname', 'CodeTerm', '-srcfolder', imageSource, '-ov', '-format', 'UDZO', dmg]);
    await invoke('codesign', ['--force', '--timestamp', '--keychain', keychain, '--sign', identity, dmg]);
    await invoke('codesign', ['--verify', '--strict', dmg]);
    await mkdir(mountpoint);
    await invoke('hdiutil', ['attach', '-readonly', '-nobrowse', '-mountpoint', mountpoint, dmg]);
    mounted = true;
    const mountedApp = path.join(mountpoint, 'CodeTerm.app');
    await verifyMacPayload({ app: mountedApp, stage, expected });
    await invoke('codesign', ['--verify', '--strict', '--deep', mountedApp]);
  } finally {
    if (mounted) await invoke('hdiutil', ['detach', mountpoint]);
    await rm(scratch, { recursive: true, force: true });
  }
  return { target, app, dmg, serverSha256: hash(expected.server), cliSha256: hash(expected.cli) };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  bundleMacPair({ target: process.argv[2] })
    .then(result => process.stdout.write(JSON.stringify(result) + '\n'))
    .catch(error => { process.stderr.write('macOS pair bundle refused: ' + error.message + '\n'); process.exitCode = 1; });
}
