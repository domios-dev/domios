#!/usr/bin/env node
// Caller: CI release-cloud-image.yml; scripts/docker/cloud-image.test.mjs; skill github-release
// Builds and minisign-signs the schema-1 headless release feed envelope (domios-headless-linux-<arch>.json)
// consumed by packaging/docker/verify.py under ARCHIVE_SOURCE=signed-release.
import { createHash, createPublicKey, verify as edVerify } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { lstat, mkdtemp, readFile, rename, rm, writeFile } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const SPKI_ED25519_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');
const MAX_ARCHIVE_BYTES = 1024 ** 3;
const PLATFORMS = new Set(['linux-x86_64', 'linux-aarch64']);

export function verifyPinnedReleasePubKey({ tauriConfText, releasePubBytes }) {
  const config = JSON.parse(tauriConfText);
  const pubkeyB64 = config?.plugins?.updater?.pubkey;
  if (typeof pubkeyB64 !== 'string' || !pubkeyB64.trim()) {
    throw new Error('src-tauri/tauri.conf.json is missing plugins.updater.pubkey');
  }
  const expected = Buffer.from(pubkeyB64.trim(), 'base64').toString('utf8').replaceAll('\r\n', '\n');
  const actual = Buffer.from(releasePubBytes).toString('utf8').replaceAll('\r\n', '\n');
  if (expected !== actual) {
    throw new Error('packaging/docker/release.pub does not match src-tauri/tauri.conf.json plugins.updater.pubkey');
  }
}

export function decodeMinisignPublicKey(publicKeyText) {
  const lines = Buffer.from(publicKeyText).toString('utf8').split(/\r?\n/).filter(line => line.trim() !== '');
  const body = lines.find(line => !line.startsWith('untrusted comment:'));
  if (!body) throw new Error('Public key is missing its base64 payload line');
  const bytes = Buffer.from(body.trim(), 'base64');
  if (bytes.length !== 42 || bytes.subarray(0, 2).toString('ascii') !== 'Ed') {
    throw new Error('Invalid minisign Ed25519 public key payload');
  }
  return {
    keyId: bytes.subarray(2, 10),
    publicKey: createPublicKey({
      key: Buffer.concat([SPKI_ED25519_PREFIX, bytes.subarray(10)]),
      format: 'der',
      type: 'spki',
    }),
  };
}

export function buildHeadlessFeedManifest({ platform, version, archiveName, repository, tag, sha256, size }) {
  if (!PLATFORMS.has(platform)) throw new Error(`Unsupported Linux headless platform: ${platform}`);
  if (!/^\d+\.\d+\.\d+$/.test(version ?? '') || tag !== `v${version}`) {
    throw new Error('Stable vMAJOR.MINOR.PATCH tag and matching version are required');
  }
  const expectedArchive = `domios-headless-v${version}-${platform}.tar.gz`;
  if (archiveName !== expectedArchive) {
    throw new Error(`Archive basename '${archiveName}' does not match expected '${expectedArchive}'`);
  }
  if (!/^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/.test(repository ?? '')) {
    throw new Error('Valid owner/repo repository is required');
  }
  if (!/^[a-f0-9]{64}$/.test(sha256 ?? '')) throw new Error('Invalid archive SHA-256');
  if (!Number.isSafeInteger(size) || size <= 0 || size > MAX_ARCHIVE_BYTES) {
    throw new Error('Archive byte size must be between 1 byte and 1 GiB');
  }
  return {
    schema: 1,
    platform,
    version,
    archive: archiveName,
    url: `https://github.com/${repository}/releases/download/${tag}/${archiveName}`,
    sha256,
    size,
    allow_downgrade: false,
  };
}

export function verifySignedHeadlessEnvelope({ envelope, expectedManifest, publicKeyBytes }) {
  if (!envelope || typeof envelope !== 'object' || Array.isArray(envelope)) {
    throw new Error('Signed envelope must be a JSON object');
  }
  if (JSON.stringify(Object.keys(envelope).sort()) !== JSON.stringify(['manifest', 'signature'])) {
    throw new Error('Signed envelope must contain exactly manifest and signature fields');
  }
  const manifestBytes = Buffer.from(envelope.manifest, 'base64');
  if (manifestBytes.toString('base64') !== envelope.manifest) {
    throw new Error('Envelope manifest is not canonical base64');
  }
  const parsedManifest = JSON.parse(manifestBytes.toString('utf8'));
  if (JSON.stringify(parsedManifest) !== JSON.stringify(expectedManifest)) {
    throw new Error('Decoded envelope manifest differs from expected release archive identity');
  }
  const sigBytes = Buffer.from(envelope.signature, 'base64');
  if (sigBytes.toString('base64') !== envelope.signature) {
    throw new Error('Envelope signature is not canonical base64');
  }
  const sigText = sigBytes.toString('utf8');
  const lines = sigText.split(/\r?\n/).filter(line => line.trim() !== '');
  if (lines.length !== 4 || !lines[0].startsWith('untrusted comment: ') || !lines[2].startsWith('trusted comment: ')) {
    throw new Error('Decoded signature is not a valid 4-line minisign signature');
  }
  const packet = Buffer.from(lines[1].trim(), 'base64');
  if (packet.length !== 74) throw new Error('Invalid minisign signature packet length');
  const algorithm = packet.subarray(0, 2).toString('ascii');
  if (algorithm !== 'ED' && algorithm !== 'Ed') throw new Error(`Unsupported minisign algorithm: ${algorithm}`);
  const { keyId, publicKey } = decodeMinisignPublicKey(publicKeyBytes);
  if (!packet.subarray(2, 10).equals(keyId)) throw new Error('Signature key ID does not match release.pub');
  const signature = packet.subarray(10);
  const message = algorithm === 'ED' ? createHash('blake2b512').update(manifestBytes).digest() : manifestBytes;
  if (!edVerify(null, message, publicKey, signature)) {
    throw new Error('Minisign payload signature verification failed');
  }
  const trustedComment = Buffer.from(lines[2].slice('trusted comment: '.length), 'utf8');
  const globalSignature = Buffer.from(lines[3].trim(), 'base64');
  if (globalSignature.length !== 64 || !edVerify(null, Buffer.concat([signature, trustedComment]), publicKey, globalSignature)) {
    throw new Error('Minisign trusted comment signature verification failed');
  }
  return parsedManifest;
}

function defaultTauriSigner(manifestPath, version) {
  const keyPath = process.env.TAURI_SIGNING_PRIVATE_KEY_PATH;
  if (!keyPath) throw new Error('TAURI_SIGNING_PRIVATE_KEY_PATH is required to sign the headless release feed');
  const env = { ...process.env };
  delete env.TAURI_SIGNING_PRIVATE_KEY;
  // Capture output so signing failures cannot leak private key material.
  try {
    execFileSync('pnpm', ['exec', 'tauri', 'signer', 'sign', '--app-version', version, manifestPath], {
      cwd: ROOT,
      env,
      stdio: ['ignore', 'pipe', 'pipe'],
      timeout: 60_000,
    });
  } catch (error) {
    throw new Error(`tauri signer sign failed (exit ${error.status ?? 'unknown'})`);
  }
  return `${manifestPath}.sig`;
}

export async function signHeadlessFeed({
  archivePath,
  tag,
  platform,
  repository,
  outputDir,
  repoRoot = ROOT,
  signer = defaultTauriSigner,
}) {
  const tauriConfText = await readFile(path.join(repoRoot, 'src-tauri', 'tauri.conf.json'), 'utf8');
  const releasePubBytes = await readFile(path.join(repoRoot, 'packaging', 'docker', 'release.pub'));
  verifyPinnedReleasePubKey({ tauriConfText, releasePubBytes });

  const info = await lstat(archivePath);
  if (!info.isFile() || info.isSymbolicLink()) throw new Error('Headless archive must be a regular file');
  const archiveBytes = await readFile(archivePath);
  const version = tag.startsWith('v') ? tag.slice(1) : '';
  const manifest = buildHeadlessFeedManifest({
    platform,
    version,
    archiveName: path.basename(archivePath),
    repository,
    tag,
    sha256: createHash('sha256').update(archiveBytes).digest('hex'),
    size: archiveBytes.length,
  });
  const manifestBytes = Buffer.from(JSON.stringify(manifest, null, 2) + '\n', 'utf8');
  const scratch = await mkdtemp(path.join(os.tmpdir(), 'domios-headless-feed-'));
  try {
    const manifestPath = path.join(scratch, 'manifest.json');
    await writeFile(manifestPath, manifestBytes);
    const sigPath = await signer(manifestPath, version);
    const sigBase64 = (await readFile(sigPath, 'utf8')).replace(/\s+/g, '');
    const envelope = {
      manifest: manifestBytes.toString('base64'),
      signature: sigBase64,
    };
    verifySignedHeadlessEnvelope({ envelope, expectedManifest: manifest, publicKeyBytes: releasePubBytes });
    const feedName = `domios-headless-${platform}.json`;
    const finalPath = path.join(outputDir, feedName);
    const tempPath = `${finalPath}.tmp.${process.pid}`;
    await writeFile(tempPath, JSON.stringify(envelope, null, 2) + '\n', 'utf8');
    await rename(tempPath, finalPath);
    return { feedPath: finalPath, feedName, manifest };
  } finally {
    await rm(scratch, { recursive: true, force: true });
  }
}

function parseArgs(argv) {
  const args = {};
  for (let i = 2; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!flag?.startsWith('--') || value === undefined) throw new Error(`Invalid argument: ${flag ?? ''}`);
    args[flag.slice(2)] = value;
  }
  for (const required of ['archive', 'tag', 'platform', 'repository', 'output']) {
    if (!args[required]) throw new Error(`Missing required flag --${required}`);
  }
  return args;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = parseArgs(process.argv);
  signHeadlessFeed({
    archivePath: args.archive,
    tag: args.tag,
    platform: args.platform,
    repository: args.repository,
    outputDir: args.output,
  })
    .then(result => {
      console.log(JSON.stringify({ feed: result.feedPath, sha256: result.manifest.sha256, size: result.manifest.size }));
    })
    .catch(error => {
      console.error(`headless feed signing refused: ${error.message}`);
      process.exitCode = 1;
    });
}
