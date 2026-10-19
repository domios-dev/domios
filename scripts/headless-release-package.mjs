#!/usr/bin/env node
import { createHash, randomUUID } from 'node:crypto';
import { lstat, mkdir, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { gunzipSync, gzipSync } from 'node:zlib';
import { SUPPORTED_BOOTSTRAP_PROTOCOL } from './package-server-component-manifest.mjs';

export const HEADLESS_TARGETS = Object.freeze({
  'darwin-aarch64': Object.freeze({ target: 'aarch64-apple-darwin', server: 'domios-server', cli: 'codeterm' }),
  'darwin-x86_64': Object.freeze({ target: 'x86_64-apple-darwin', server: 'domios-server', cli: 'codeterm' }),
  'linux-aarch64': Object.freeze({ target: 'aarch64-unknown-linux-gnu', server: 'domios-server', cli: 'codeterm' }),
  'linux-x86_64': Object.freeze({ target: 'x86_64-unknown-linux-gnu', server: 'domios-server', cli: 'codeterm' }),
  'windows-aarch64': Object.freeze({ target: 'aarch64-pc-windows-msvc', server: 'domios-server.exe', cli: 'codeterm.exe', conpty: 'arm64' }),
  'windows-x86_64': Object.freeze({ target: 'x86_64-pc-windows-msvc', server: 'domios-server.exe', cli: 'codeterm.exe', conpty: 'x64' }),
});

const TARGETS_BY_TRIPLE = new Map(Object.entries(HEADLESS_TARGETS).map(([platform, value]) => [value.target, { platform, ...value }]));
const RECEIPT_FIELDS = ['schema_version', 'platform', 'target', 'tag', 'version', 'source_sha', 'files'];
const COMPONENT_FIELDS = ['schema', 'server_build', 'server_executable', 'server_sha256', 'client_identity', 'api_version', 'bootstrap_protocol'];
const SHA256_PATTERN = /^[a-f0-9]{64}$/;
const VERSION_PATTERN = /^\d+\.\d+\.\d+$/;
const TAG_PATTERN = /^v\d+\.\d+\.\d+$/;
const SOURCE_SHA_PATTERN = /^[a-f0-9]{40}$/;
const RECEIPT_NAME = 'headless-stage.json';

function fail(message) {
  throw new Error(message);
}

function digest(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

function compareUtf8(left, right) {
  return Buffer.compare(Buffer.from(left, 'utf8'), Buffer.from(right, 'utf8'));
}

function exactFields(value, fields, label) {
  if (value === null || typeof value !== 'object' || Array.isArray(value) ||
      Object.keys(value).length !== fields.length || fields.some(field => !Object.hasOwn(value, field))) {
    fail(label + ' fields do not match the reviewed schema');
  }
}

function validateIdentity({ target, tag, version, sourceSha }) {
  if (!TARGETS_BY_TRIPLE.has(target)) fail('unsupported Rust target ' + (target ?? ''));
  if (!VERSION_PATTERN.test(version ?? '') || !TAG_PATTERN.test(tag ?? '') || tag !== 'v' + version) {
    fail('tag and version must be an explicit matching vMAJOR.MINOR.PATCH pair');
  }
  if (!SOURCE_SHA_PATTERN.test(sourceSha ?? '')) fail('source SHA must be an explicit lowercase 40-character commit SHA');
  return TARGETS_BY_TRIPLE.get(target);
}

function validateRelativePath(value, label = 'payload path') {
  if (typeof value !== 'string' || value.length === 0 || value.includes('\\') || value.includes('\0') ||
      value.includes(':') || value.startsWith('/') || /[\u0000-\u001f\u007f]/.test(value)) {
    fail(label + ' is not a safe POSIX relative path');
  }
  const segments = value.split('/');
  if (segments.some(segment => segment === '' || segment === '.' || segment === '..') ||
      segments.some(segment => /^(?:con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i.test(segment))) {
    fail(label + ' contains an unsafe path segment');
  }
  return value;
}

function isPayloadDirectory(relativePath, metadata) {
  if (relativePath === 'domios-server' || relativePath === 'cli') return true;
  if (relativePath === 'dist' || relativePath.startsWith('dist/')) return true;
  if (relativePath === 'skills' || relativePath.startsWith('skills/')) return true;
  if (!metadata.conpty) return false;
  return relativePath === 'conpty' || relativePath === 'conpty/' + metadata.conpty;
}

function serverPath(metadata) {
  return 'domios-server/' + metadata.server;
}

function cliPath(metadata) {
  return 'cli/' + metadata.cli;
}

function isPayloadFile(relativePath, metadata) {
  if (relativePath === 'LICENSE' || relativePath === 'NOTICE' || relativePath === 'server-component.json') return true;
  if (relativePath === serverPath(metadata) || relativePath === cliPath(metadata)) return true;
  if (relativePath.startsWith('dist/') || relativePath.startsWith('skills/')) return true;
  if (!metadata.conpty) return false;
  return [
    'conpty/' + metadata.conpty + '/OpenConsole.exe',
    'conpty/' + metadata.conpty + '/conpty.dll',
  ].includes(relativePath);
}

function assertReferencedFrontendFiles(indexBytes, paths) {
  const html = indexBytes.toString('utf8');
  const tags = html.matchAll(/<(?:script|link)\b[^>]*>/gi);
  const references = new Set();
  for (const [tag] of tags) {
    for (const match of tag.matchAll(/\b(?:src|href)\s*=\s*(["'])(.*?)\1/gi)) {
      const raw = match[2].trim();
      if (!raw || raw.startsWith('#') || raw.startsWith('//') || /^[a-z][a-z\d+.-]*:/i.test(raw)) continue;
      let pathname;
      try {
        pathname = decodeURIComponent(raw.split(/[?#]/, 1)[0]);
      } catch {
        fail('frontend index has an invalid asset reference ' + raw);
      }
      if (!pathname || pathname === '/') continue;
      const relative = pathname.startsWith('/') ? pathname.slice(1) : pathname;
      const resolved = path.posix.normalize('dist/' + relative);
      if (!resolved.startsWith('dist/') || resolved === 'dist/..') fail('frontend index has an unsafe asset reference ' + raw);
      references.add(resolved);
    }
  }
  for (const reference of references) {
    if (!paths.has(reference)) fail('frontend asset referenced by dist/index.html is missing: ' + reference);
  }
}

async function readStageInventory(stageDirectory, metadata) {
  const discovered = new Map();
  async function visit(directory, relativeDirectory = '') {
    const entries = (await readdir(directory)).sort(compareUtf8);
    for (const name of entries) {
      const relativePath = relativeDirectory ? relativeDirectory + '/' + name : name;
      validateRelativePath(relativePath);
      const absolutePath = path.join(directory, name);
      const stat = await lstat(absolutePath);
      if (stat.isSymbolicLink()) fail('stage contains a symlink: ' + relativePath);
      if (stat.isDirectory()) {
        if (!isPayloadDirectory(relativePath, metadata)) fail('stage contains an extra directory: ' + relativePath);
        await visit(absolutePath, relativePath);
        continue;
      }
      if (!stat.isFile()) fail('stage contains a non-regular file: ' + relativePath);
      if (relativePath === RECEIPT_NAME) continue;
      if (!isPayloadFile(relativePath, metadata)) fail('stage contains an extra file: ' + relativePath);
      const bytes = await readFile(absolutePath);
      const executable = relativePath === serverPath(metadata) ||
        relativePath === cliPath(metadata) ||
        relativePath.endsWith('/OpenConsole.exe');
      discovered.set(relativePath, {
        path: relativePath,
        bytes,
        mode: executable || (stat.mode & 0o111) ? 0o755 : 0o644,
        sha256: digest(bytes),
      });
    }
  }
  await visit(stageDirectory);
  return discovered;
}

function validateComponentManifest(bytes, metadata, discovered) {
  let manifest;
  try {
    manifest = JSON.parse(bytes.toString('utf8'));
  } catch {
    fail('server-component.json is not valid JSON');
  }
  exactFields(manifest, COMPONENT_FIELDS, 'server-component.json');
  if (manifest.schema !== 2) fail('server-component.json schema must be 2');
  if (typeof manifest.server_build !== 'string' || manifest.server_build.length === 0 ||
      manifest.server_build.trim() !== manifest.server_build || /[\u0000-\u001f\u007f]/.test(manifest.server_build)) {
    fail('server-component.json server_build must identify the built server');
  }
  if (manifest.server_executable !== serverPath(metadata)) {
    fail('server-component.json server_executable does not match the staged server path');
  }
  if (!SHA256_PATTERN.test(manifest.server_sha256 ?? '')) {
    fail('server-component.json server_sha256 must be a lowercase SHA-256 digest');
  }
  if (JSON.stringify(manifest.client_identity) !== JSON.stringify({ kind: 'none' })) {
    fail('headless server-component.json must declare no native client identity');
  }
  if (manifest.api_version !== 3) fail('server-component.json api_version must be 3');
  if (manifest.bootstrap_protocol !== SUPPORTED_BOOTSTRAP_PROTOCOL) {
    fail('server-component.json bootstrap_protocol must be ' + SUPPORTED_BOOTSTRAP_PROTOCOL);
  }
  const server = discovered.get(serverPath(metadata));
  if (!server) fail('staged server executable is missing');
  if (manifest.server_sha256 !== server.sha256) {
    fail('server-component.json server_sha256 does not match the staged server bytes');
  }
}

export async function validateHeadlessStage({ stageDirectory, target, tag, version, sourceSha }) {
  const metadata = validateIdentity({ target, tag, version, sourceSha });
  const stageRoot = path.resolve(stageDirectory ?? '');
  if (!stageDirectory || path.basename(stageRoot) !== target) fail('stage directory name must equal the explicit Rust target');
  const rootStat = await lstat(stageRoot).catch(() => null);
  if (!rootStat?.isDirectory() || rootStat.isSymbolicLink()) {
    fail('common stage directory is missing or not a regular directory: ' + stageRoot);
  }
  const receiptPath = path.join(stageRoot, RECEIPT_NAME);
  const receiptStat = await lstat(receiptPath).catch(() => null);
  if (!receiptStat?.isFile() || receiptStat.isSymbolicLink()) {
    fail('common stage receipt is missing or not a regular file: ' + receiptPath);
  }
  let receipt;
  try {
    receipt = JSON.parse(await readFile(receiptPath, 'utf8'));
  } catch {
    fail('headless-stage.json is not valid JSON');
  }
  exactFields(receipt, RECEIPT_FIELDS, 'headless-stage.json');
  if (receipt.schema_version !== 1) fail('headless-stage.json schema_version must be 1');
  if (receipt.platform !== metadata.platform) fail('stage platform does not match the Rust target');
  if (receipt.target !== target || receipt.target !== path.basename(stageRoot)) {
    fail('stage target does not match the explicit Rust target and directory');
  }
  if (receipt.tag !== tag || receipt.version !== version || receipt.source_sha !== sourceSha) {
    fail('stage tag, version, or source SHA does not match explicit release inputs');
  }
  if (!Array.isArray(receipt.files) || receipt.files.length === 0) fail('stage files inventory must be non-empty');
  let previousPath;
  for (const row of receipt.files) {
    exactFields(row, ['path', 'sha256'], 'headless-stage.json file row');
    validateRelativePath(row.path, 'inventory path');
    if (!SHA256_PATTERN.test(row.sha256 ?? '')) fail('invalid SHA-256 for ' + row.path);
    if (previousPath !== undefined && compareUtf8(previousPath, row.path) >= 0) {
      fail('stage files must be UTF-8 byte sorted and unique');
    }
    if (!isPayloadFile(row.path, metadata)) fail('stage inventory has an unsupported payload path: ' + row.path);
    previousPath = row.path;
  }

  const discovered = await readStageInventory(stageRoot, metadata);
  const expectedPaths = receipt.files.map(row => row.path);
  const actualPaths = [...discovered.keys()].sort(compareUtf8);
  if (JSON.stringify(expectedPaths) !== JSON.stringify(actualPaths)) {
    fail('stage files inventory must list every staged payload file exactly once');
  }
  for (const row of receipt.files) {
    if (discovered.get(row.path).sha256 !== row.sha256) fail('stage SHA-256 does not match payload bytes: ' + row.path);
  }

  const required = [serverPath(metadata), 'server-component.json', cliPath(metadata), 'dist/index.html', 'LICENSE', 'NOTICE'];
  if (metadata.conpty) {
    required.push('conpty/' + metadata.conpty + '/OpenConsole.exe', 'conpty/' + metadata.conpty + '/conpty.dll');
  }
  for (const file of required) if (!discovered.has(file)) fail('required headless payload is missing: ' + file);
  if (![...discovered.keys()].some(file => file.startsWith('skills/'))) fail('required skills resources are missing');
  if (![...discovered.keys()].some(file => file.startsWith('dist/assets/'))) {
    fail('frontend build assets are missing from dist/assets');
  }
  assertReferencedFrontendFiles(discovered.get('dist/index.html').bytes, discovered);
  validateComponentManifest(discovered.get('server-component.json').bytes, metadata, discovered);

  return {
    stageDirectory: stageRoot,
    receipt,
    platform: metadata.platform,
    files: [...discovered.values()].sort((left, right) => compareUtf8(left.path, right.path)),
  };
}

function writeOctal(bytes, value, offset, length) {
  const encoded = value.toString(8).padStart(length - 1, '0') + '\0';
  if (encoded.length > length) fail('payload is too large for deterministic USTAR');
  bytes.write(encoded, offset, length, 'ascii');
}

function splitUstarPath(relativePath) {
  const encoded = Buffer.from(relativePath, 'utf8');
  if (encoded.length <= 100) return { name: encoded, prefix: Buffer.alloc(0) };
  const choices = [];
  for (let index = relativePath.lastIndexOf('/'); index > 0; index = relativePath.lastIndexOf('/', index - 1)) {
    const prefix = Buffer.from(relativePath.slice(0, index), 'utf8');
    const name = Buffer.from(relativePath.slice(index + 1), 'utf8');
    if (prefix.length <= 155 && name.length <= 100) choices.push({ name, prefix });
  }
  if (!choices.length) fail('archive path exceeds USTAR limits: ' + relativePath);
  return choices[0];
}

function ustarHeader(file) {
  const bytes = Buffer.alloc(512);
  const { name, prefix } = splitUstarPath(file.path);
  name.copy(bytes, 0);
  writeOctal(bytes, file.mode, 100, 8);
  writeOctal(bytes, 0, 108, 8);
  writeOctal(bytes, 0, 116, 8);
  writeOctal(bytes, file.bytes.length, 124, 12);
  writeOctal(bytes, 0, 136, 12);
  bytes.fill(32, 148, 156);
  bytes[156] = '0'.charCodeAt(0);
  bytes.write('ustar\0', 257, 'ascii');
  bytes.write('00', 263, 'ascii');
  prefix.copy(bytes, 345);
  const checksum = bytes.reduce((sum, value) => sum + value, 0);
  bytes.write(checksum.toString(8).padStart(6, '0') + '\0 ', 148, 8, 'ascii');
  return bytes;
}

function createArchive(files) {
  const chunks = [];
  for (const file of files) {
    chunks.push(ustarHeader(file), file.bytes);
    const padding = (512 - (file.bytes.length % 512)) % 512;
    if (padding) chunks.push(Buffer.alloc(padding));
  }
  chunks.push(Buffer.alloc(1024));
  const gzip = gzipSync(Buffer.concat(chunks), { level: 9 });
  gzip.fill(0, 4, 8);
  gzip[9] = 255;
  return gzip;
}

function parseOctal(bytes, offset, length, label) {
  const value = bytes.subarray(offset, offset + length).toString('ascii').replace(/\0.*$/, '').trim();
  if (!/^[0-7]+$/.test(value)) fail('archive has invalid ' + label);
  return Number.parseInt(value, 8);
}

export function readHeadlessArchive(archiveBytes) {
  const tar = gunzipSync(archiveBytes);
  const files = new Map();
  let offset = 0;
  let zeroBlocks = 0;
  while (offset + 512 <= tar.length) {
    const header = tar.subarray(offset, offset + 512);
    if (header.every(byte => byte === 0)) {
      zeroBlocks += 1;
      offset += 512;
      if (zeroBlocks === 2) break;
      continue;
    }
    if (zeroBlocks !== 0) fail('archive has data after its first end marker');
    const savedChecksum = parseOctal(header, 148, 8, 'header checksum');
    const checkHeader = Buffer.from(header);
    checkHeader.fill(32, 148, 156);
    if (savedChecksum !== checkHeader.reduce((sum, byte) => sum + byte, 0)) fail('archive header checksum is invalid');
    if (header[156] !== 0 && header[156] !== '0'.charCodeAt(0)) fail('archive contains a non-regular entry');
    const name = header.subarray(0, 100).toString('utf8').replace(/\0.*$/, '');
    const prefix = header.subarray(345, 500).toString('utf8').replace(/\0.*$/, '');
    const relativePath = validateRelativePath(prefix ? prefix + '/' + name : name, 'archive path');
    const size = parseOctal(header, 124, 12, 'file size');
    const mode = parseOctal(header, 100, 8, 'file mode');
    const dataOffset = offset + 512;
    const end = dataOffset + size;
    if (end > tar.length || files.has(relativePath)) fail('archive file data is truncated or duplicated');
    files.set(relativePath, { bytes: Buffer.from(tar.subarray(dataOffset, end)), mode });
    offset = dataOffset + Math.ceil(size / 512) * 512;
  }
  if (zeroBlocks !== 2 || offset !== tar.length) fail('archive has a missing or malformed end marker');
  if (tar.subarray(offset).some(byte => byte !== 0)) fail('archive has non-zero trailing bytes');
  return files;
}

export function headlessAssetNames(tag, platform) {
  if (!TAG_PATTERN.test(tag ?? '') || !Object.hasOwn(HEADLESS_TARGETS, platform ?? '')) {
    fail('cannot name headless assets for an unsupported tag or platform');
  }
  const archive = 'domios-headless-' + tag + '-' + platform + '.tar.gz';
  return { archive, checksum: archive + '.sha256', manifest: archive + '.manifest.json' };
}

function ensureArchiveReadback(archive, expectedFiles) {
  const actual = readHeadlessArchive(archive);
  if (actual.size !== expectedFiles.length) fail('headless archive readback has an unexpected file count');
  for (const file of expectedFiles) {
    const entry = actual.get(file.path);
    if (!entry || !entry.bytes.equals(file.bytes) || entry.mode !== file.mode) {
      fail('headless archive readback differs from the verified stage: ' + file.path);
    }
  }
}

async function finalFileExists(file) {
  try {
    const stat = await lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink()) fail('existing headless output is not a regular file: ' + file);
    return true;
  } catch (error) {
    if (error.code === 'ENOENT') return false;
    throw error;
  }
}

async function replacePackageAssets(temporaryPaths, finalPaths, backupPaths, createdTemporaryPaths) {
  const existing = [];
  for (const file of finalPaths) existing.push(await finalFileExists(file));
  const backedUp = [];
  const installed = [];
  try {
    for (let index = 0; index < finalPaths.length; index += 1) {
      if (!existing[index]) continue;
      await rename(finalPaths[index], backupPaths[index]);
      backedUp.push(index);
    }
    for (let index = 0; index < finalPaths.length; index += 1) {
      await rename(temporaryPaths[index], finalPaths[index]);
      createdTemporaryPaths.delete(temporaryPaths[index]);
      installed.push(index);
    }
  } catch (error) {
    const rollbackErrors = [];
    for (const index of installed.reverse()) {
      try {
        await rm(finalPaths[index], { force: true });
      } catch (rollbackError) {
        rollbackErrors.push(rollbackError);
      }
    }
    for (const index of backedUp.reverse()) {
      try {
        await rename(backupPaths[index], finalPaths[index]);
      } catch (rollbackError) {
        rollbackErrors.push(rollbackError);
      }
    }
    if (rollbackErrors.length) {
      throw new AggregateError([error, ...rollbackErrors], 'headless output replacement failed and rollback was incomplete');
    }
    throw error;
  }
  for (const index of backedUp) {
    try {
      await rm(backupPaths[index], { force: true });
    } catch (error) {
      process.stderr.write('headless package committed; could not clear prior-output backup: ' + backupPaths[index] + '\n');
    }
  }
}

export async function packageHeadless({ stageDirectory, outputDirectory, target, tag, version, sourceSha }) {
  const { platform } = validateIdentity({ target, tag, version, sourceSha });
  if (!outputDirectory) fail('headless output directory is required');
  const names = headlessAssetNames(tag, platform);
  const outputRoot = path.resolve(outputDirectory);
  const stageRoot = path.resolve(stageDirectory ?? '');
  const relativeOutput = path.relative(stageRoot, outputRoot);
  if (relativeOutput === '' || (!relativeOutput.startsWith('..') && !path.isAbsolute(relativeOutput))) {
    fail('headless output directory must be outside the common stage');
  }
  await mkdir(outputRoot, { recursive: true });
  const outputStat = await lstat(outputRoot);
  if (!outputStat.isDirectory() || outputStat.isSymbolicLink()) fail('headless output path must be a regular directory');
  const finalPaths = [names.archive, names.checksum, names.manifest].map(name => path.join(outputRoot, name));
  const temporaryPaths = finalPaths.map(file => file + '.' + randomUUID() + '.tmp');
  const backupPaths = finalPaths.map(file => file + '.' + randomUUID() + '.bak');
  const createdTemporaryPaths = new Set();
  try {
    const stage = await validateHeadlessStage({ stageDirectory, target, tag, version, sourceSha });
    await writeFile(temporaryPaths[0], createArchive(stage.files), { flag: 'wx' });
    createdTemporaryPaths.add(temporaryPaths[0]);
    const archive = await readFile(temporaryPaths[0]);
    ensureArchiveReadback(archive, stage.files);
    const archiveSha256 = digest(archive);
    const manifest = {
      schema_version: 1,
      variant: 'headless',
      platform,
      target,
      tag,
      version,
      source_sha: sourceSha,
      archive: names.archive,
      checksum: names.checksum,
      archive_sha256: archiveSha256,
    };
    const checksumText = archiveSha256 + '  ' + names.archive + '\n';
    const manifestText = JSON.stringify(manifest, null, 2) + '\n';
    await writeFile(temporaryPaths[1], checksumText, { flag: 'wx' });
    createdTemporaryPaths.add(temporaryPaths[1]);
    await writeFile(temporaryPaths[2], manifestText, { flag: 'wx' });
    createdTemporaryPaths.add(temporaryPaths[2]);
    const checksumReadback = await readFile(temporaryPaths[1], 'utf8');
    const manifestReadback = JSON.parse(await readFile(temporaryPaths[2], 'utf8'));
    if (checksumReadback !== checksumText || manifestReadback.archive_sha256 !== digest(await readFile(temporaryPaths[0]))) {
      fail('headless checksum or metadata readback failed');
    }
    await replacePackageAssets(temporaryPaths, finalPaths, backupPaths, createdTemporaryPaths);
    return {
      ...manifest,
      archivePath: finalPaths[0],
      checksumPath: finalPaths[1],
      manifestPath: finalPaths[2],
    };
  } finally {
    await Promise.all([...createdTemporaryPaths].map(file => rm(file, { force: true })));
  }
}

function parseArguments(args) {
  const allowed = new Set(['--stage-dir', '--output-dir', '--target', '--tag', '--version', '--source-sha']);
  const values = new Map();
  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
    if (!allowed.has(name) || value === undefined || value.startsWith('--')) fail('invalid or incomplete argument: ' + (name ?? ''));
    if (values.has(name)) fail('duplicate argument: ' + name);
    values.set(name, value);
  }
  for (const name of allowed) if (!values.has(name)) fail('missing required argument: ' + name);
  return {
    stageDirectory: values.get('--stage-dir'),
    outputDirectory: values.get('--output-dir'),
    target: values.get('--target'),
    tag: values.get('--tag'),
    version: values.get('--version'),
    sourceSha: values.get('--source-sha'),
  };
}

async function main() {
  try {
    const artifacts = await packageHeadless(parseArguments(process.argv.slice(2)));
    process.stdout.write(artifacts.archivePath + '\n' + artifacts.checksumPath + '\n' + artifacts.manifestPath + '\n');
  } catch (error) {
    process.stderr.write('headless release packaging refused: ' + error.message + '\n');
    process.exitCode = 1;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) main();
