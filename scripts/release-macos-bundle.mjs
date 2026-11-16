// Caller: github-release/scripts/build-mac.sh, release-macos-x86_64.sh
import { createHash } from 'node:crypto';
import { execFileSync, spawnSync } from 'node:child_process';
import { lstat, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { validateManifest } from './package-server-component-manifest.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const hash = bytes => createHash('sha256').update(bytes).digest('hex');
const commandTimeoutMs = 240000;
const createCaptureBytes = 1048576;
const diagnosticTimeoutMs = 5000;
const diagnosticOutputBytes = 65536;

async function regularBytes(file) {
  const stat = await lstat(file);
  if (!stat.isFile() || stat.isSymbolicLink() || !stat.size) throw new Error('expected regular package file: ' + file);
  return readFile(file);
}

export async function verifyMacPayload({ app, stage, expected }) {
  const names = (await readdir(path.join(app, 'Contents', 'MacOS'))).sort();
  if (JSON.stringify(names) !== JSON.stringify(['domios-app', 'domios-server'])) {
    throw new Error('unexpected macOS executable inventory: ' + names.join(', '));
  }
  for (const [relative, bytes] of [
    ['Contents/MacOS/domios-server', expected.server],
    ['Contents/Resources/cli/codeterm', expected.cli],
    ['Contents/Resources/cli/domios', expected.cli],
    ['Contents/Resources/server-component.json', expected.manifest],
  ]) {
    if (!(await regularBytes(path.join(app, relative))).equals(bytes)) {
      throw new Error('macOS bundling changed frozen bytes: ' + relative + ' (stage ' + stage + ')');
    }
  }
  await regularBytes(path.join(app, 'Contents', 'MacOS', 'domios-app'));
}

function command(program, args, cwd, options = {}) {
  return execFileSync(program, args, { cwd, stdio: 'inherit', timeout: commandTimeoutMs, ...options });
}

async function recordDmgFailure({ repo, target, sourceSha, imageSource, dmg, args, error }) {
  const directory = path.join(repo, 'release-artifacts', 'macos-dmg-diagnostics', target);
  await mkdir(directory, { recursive: true });
  const text = value => Buffer.from(value ?? '').subarray(0, diagnosticOutputBytes).toString('utf8');
  await writeFile(path.join(directory, 'create.json'), JSON.stringify({
    source_sha: sourceSha, target, program: 'hdiutil', args,
    timeout_ms: commandTimeoutMs, capture_limit_bytes: createCaptureBytes,
    retained_stream_limit_bytes: diagnosticOutputBytes,
    status: error.status ?? null, signal: error.signal ?? null, code: error.code ?? null,
    message: String(error.message ?? error).slice(0, 4096),
    stdout: text(error.stdout), stderr: text(error.stderr),
  }, null, 2) + '\n');
  for (const [name, program, diagnosticArgs] of [
    ['images', 'hdiutil', ['info', '-plist']],
    ['disks', 'diskutil', ['list', '-plist']],
    ['source-holders', 'lsof', ['-nP', '-Fpcn', '+D', imageSource]],
    ['output-holders', 'lsof', ['-nP', '-Fpcn', '--', dmg]],
  ]) {
    try {
      const result = spawnSync(program, diagnosticArgs, {
        cwd: repo, timeout: diagnosticTimeoutMs, killSignal: 'SIGKILL', maxBuffer: diagnosticOutputBytes,
      });
      await writeFile(path.join(directory, `${name}.json`), JSON.stringify({
        program, args: diagnosticArgs, timeout_ms: diagnosticTimeoutMs, output_limit_bytes: diagnosticOutputBytes,
        status: result.status, signal: result.signal, error: result.error?.message ?? null,
        stdout: text(result.stdout), stderr: text(result.stderr),
      }, null, 2) + '\n');
    } catch (diagnosticError) {
      process.stderr.write(`macOS DMG diagnostic ${name} unavailable: ${diagnosticError.message}\n`);
    }
  }
  process.stderr.write(`macOS DMG failure diagnostics: ${directory}\n`);
}

async function createDmg({ invoke, repo, target, sourceSha, imageSource, dmg }) {
  const args = ['create', '-volname', 'Domios', '-srcfolder', imageSource, '-ov', '-format', 'UDZO', dmg];
  try {
    const output = await invoke('hdiutil', args, { stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: createCaptureBytes });
    if (output?.length) process.stderr.write(output);
  } catch (error) {
    try {
      await recordDmgFailure({ repo, target, sourceSha, imageSource, dmg, args, error });
    } catch (diagnosticError) {
      process.stderr.write(`macOS DMG diagnostics unavailable: ${diagnosticError.message}\n`);
    }
    throw error;
  }
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
  const cargoTarget = env.CARGO_TARGET_DIR ? path.resolve(repo, env.CARGO_TARGET_DIR) : path.join(repo, 'src-tauri', 'target');
  const release = path.join(cargoTarget, ...targetArgs.slice(1), 'release');
  const bundleRoot = path.join(release, 'bundle');
  const app = path.join(bundleRoot, 'macos', 'Domios.app');
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
  const invoke = async (program, args, options) => run(program, args, repo, options);
  await invoke('pnpm', ['tauri', 'bundle', '--no-sign', '--no-binary-patching', ...targetArgs,
    ...(env.CODETERM_RELEASE_FEATURES ? ['--features', env.CODETERM_RELEASE_FEATURES] : []), '--bundles', 'app',
    '--config', '{"bundle":{"createUpdaterArtifacts":false}}']);
  await verifyMacPayload({ app, stage, expected });
  // Signing the outer app without --deep preserves the pre-signed CLI/server bytes.
  await invoke('codesign', ['--force', '--options', 'runtime', '--timestamp', '--keychain', keychain,
    '--entitlements', path.join(repo, 'src-tauri', 'Entitlements.plist'), '--sign', identity, app]);
  await verifyMacPayload({ app, stage, expected });
  await invoke('codesign', ['--verify', '--strict', '--deep',
    '--test-requirement', '=' + manifest.client_identity.requirement, app]);
  const arch = target.startsWith('aarch64') ? 'aarch64' : 'x64';
  const dmg = path.join(bundleRoot, 'dmg', `Domios_${version}_${arch}.dmg`);
  await mkdir(path.dirname(dmg), { recursive: true });
  const scratch = await mkdtemp(path.join(bundleRoot, '.pair-dmg-'));
  let mounted = false;
  let failure;
  const mountpoint = path.join(scratch, 'readback');
  try {
    const imageSource = path.join(scratch, 'source');
    await mkdir(imageSource);
    await invoke('ditto', ['--rsrc', '--extattr', app, path.join(imageSource, 'Domios.app')]);
    await linkApplications(path.join(imageSource, 'Applications'));
    await invoke('sync', []);
    await createDmg({ invoke, repo, target, sourceSha, imageSource, dmg });
    await invoke('codesign', ['--force', '--timestamp', '--keychain', keychain, '--sign', identity, dmg]);
    await invoke('codesign', ['--verify', '--strict', dmg]);
    await mkdir(mountpoint);
    await invoke('hdiutil', ['attach', '-readonly', '-nobrowse', '-mountpoint', mountpoint, dmg]);
    mounted = true;
    const mountedApp = path.join(mountpoint, 'Domios.app');
    await verifyMacPayload({ app: mountedApp, stage, expected });
    await invoke('codesign', ['--verify', '--strict', '--deep', mountedApp]);
  } catch (error) {
    failure = error;
    throw error;
  } finally {
    try {
      if (mounted) await invoke('hdiutil', ['detach', mountpoint]);
      await rm(scratch, { recursive: true, force: true });
    } catch (cleanupError) {
      if (!failure) throw cleanupError;
      process.stderr.write(`macOS DMG cleanup failed: ${cleanupError.message}\n`);
    }
  }
  return { target, app, dmg, serverSha256: hash(expected.server), cliSha256: hash(expected.cli) };
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  bundleMacPair({ target: process.argv[2] })
    .then(result => process.stdout.write(JSON.stringify(result) + '\n'))
    .catch(error => { process.stderr.write('macOS pair bundle refused: ' + error.message + '\n'); process.exitCode = 1; });
}
