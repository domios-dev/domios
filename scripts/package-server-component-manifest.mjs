import { createHash, randomUUID } from 'node:crypto';
import { lstat, mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export const SERVER_COMPONENT_SCHEMA = 2;
export const SUPPORTED_BOOTSTRAP_PROTOCOL = 2;
export const MANIFEST_RELATIVE_PATH = 'target/domios-package/server-component.json';

const STAGE_ROOT = path.join('target', 'domios-package');
const SERVER_RESOURCE_PREFIX = 'domios-server/';
const UINT32_MAX = 0xffff_ffff;
const FIELD_NAMES = [
  'schema',
  'server_build',
  'server_executable',
  'server_sha256',
  'client_identity',
  'api_version',
  'bootstrap_protocol',
];
const SHA256_PATTERN = /^[a-f0-9]{64}$/;

function fail(message) {
  throw new Error(message);
}

function parseVersion(value, name) {
  if (typeof value !== 'string' || !/^[1-9]\d*$/.test(value)) {
    fail(`${name} must be a positive u32`);
  }

  const parsed = Number(value);
  if (!Number.isSafeInteger(parsed) || parsed > UINT32_MAX) {
    fail(`${name} must be a positive u32`);
  }

  return parsed;
}

function validateServerExecutable(value, identity) {
  if (identity?.kind === 'macos_designated_requirement') {
    if (value === '../MacOS/domios-server') return value;
    fail('macOS app server executable must be the exact signed nested-code path');
  }
  if (
    typeof value !== 'string' ||
    value.length === 0 ||
    value.includes('\\') ||
    value.includes('\0') ||
    value.includes(':') ||
    path.posix.isAbsolute(value) ||
    path.win32.isAbsolute(value) ||
    !value.startsWith(SERVER_RESOURCE_PREFIX)
  ) {
    fail('server executable must be a relative path under resource_dir/domios-server');
  }

  const segments = value.split('/');
  if (segments.some(segment => segment.length === 0 || segment === '.' || segment === '..')) {
    fail('server executable must not contain empty, dot, or parent path segments');
  }

  return value;
}

function validateClientIdentity(identity) {
  if (identity === null || typeof identity !== 'object' || Array.isArray(identity)) fail('client identity must be an object');
  const fields = Object.keys(identity).sort();
  if (identity.kind === 'none' && JSON.stringify(fields) === JSON.stringify(['kind'])) return identity;
  if (identity.kind === 'sha256' && JSON.stringify(fields) === JSON.stringify(['kind', 'sha256']) && SHA256_PATTERN.test(identity.sha256)) return identity;
  if (identity.kind === 'macos_designated_requirement' &&
      JSON.stringify(fields) === JSON.stringify(['identifier', 'kind', 'requirement', 'team_id']) &&
      /^[A-Z0-9]{10}$/.test(identity.team_id) && identity.identifier === 'com.domios.app' &&
      identity.requirement === `anchor apple generic and identifier "com.domios.app" and certificate leaf[subject.OU] = "${identity.team_id}"`) return identity;
  fail('client identity does not match a supported ServerComponentManifest v2 variant');
}

function isPositiveU32(value) {
  return Number.isInteger(value) && value > 0 && value <= UINT32_MAX;
}

export function validateManifest(manifest) {
  if (manifest === null || typeof manifest !== 'object' || Array.isArray(manifest)) {
    fail('manifest must be an object');
  }

  if (JSON.stringify(Object.keys(manifest).sort()) !== JSON.stringify([...FIELD_NAMES].sort())) {
    fail('manifest fields do not match ServerComponentManifest v2');
  }

  if (manifest.schema !== SERVER_COMPONENT_SCHEMA) {
    fail(`schema must be ${SERVER_COMPONENT_SCHEMA}`);
  }

  if (
    typeof manifest.server_build !== 'string' ||
    manifest.server_build.length === 0 ||
    manifest.server_build.trim() !== manifest.server_build ||
    /[\u0000-\u001f\u007f]/.test(manifest.server_build)
  ) {
    fail('server build must be a non-empty printable string');
  }

  validateClientIdentity(manifest.client_identity);
  validateServerExecutable(manifest.server_executable, manifest.client_identity);

  if (!SHA256_PATTERN.test(manifest.server_sha256)) {
    fail('server_sha256 must be a lowercase SHA-256 digest');
  }

  if (!isPositiveU32(manifest.api_version)) {
    fail('api_version must be a positive u32');
  }

  if (manifest.bootstrap_protocol !== SUPPORTED_BOOTSTRAP_PROTOCOL) {
    fail(`bootstrap_protocol must be ${SUPPORTED_BOOTSTRAP_PROTOCOL}`);
  }

  return manifest;
}

function samePath(left, right) {
  const resolvedLeft = path.resolve(left);
  const resolvedRight = path.resolve(right);
  return process.platform === 'win32'
    ? resolvedLeft.toLowerCase() === resolvedRight.toLowerCase()
    : resolvedLeft === resolvedRight;
}

async function readStagedFile(filePath, name) {
  let metadata;
  try {
    metadata = await lstat(filePath);
  } catch {
    fail(`${name} does not exist: ${filePath}`);
  }

  if (!metadata.isFile() || metadata.size === 0) {
    fail(`${name} must be a non-empty regular file: ${filePath}`);
  }

  return readFile(filePath);
}

function digest(bytes) {
  return createHash('sha256').update(bytes).digest('hex');
}

export async function createManifest({
  cwd = process.cwd(),
  serverFile,
  clientFile,
  clientIdentity,
  serverBuild,
  serverExecutable,
  apiVersion,
  bootstrapProtocol,
}) {
  const outputPath = path.resolve(cwd, MANIFEST_RELATIVE_PATH);
  await rm(outputPath, { force: true });

  if (typeof serverBuild !== 'string' || serverBuild.length === 0) {
    fail('server build is required');
  }

  const serverExecutablePath = validateServerExecutable(serverExecutable, clientIdentity);
  if (!Number.isInteger(apiVersion) || !isPositiveU32(apiVersion)) {
    fail('api_version must be a positive u32');
  }

  if (bootstrapProtocol !== SUPPORTED_BOOTSTRAP_PROTOCOL) {
    fail(`bootstrap_protocol must be ${SUPPORTED_BOOTSTRAP_PROTOCOL}`);
  }

  if (typeof serverFile !== 'string' || serverFile.length === 0) {
    fail('staged server file is required');
  }

  if (!clientIdentity && (typeof clientFile !== 'string' || clientFile.length === 0)) {
    fail('staged client file is required for a SHA identity');
  }

  const stageRoot = path.resolve(cwd, STAGE_ROOT);
  const stagedServer = path.resolve(cwd, serverFile);
  const stagedClient = clientFile ? path.resolve(cwd, clientFile) : null;
  const expectedServer = path.resolve(stageRoot, 'domios-server', path.basename(serverExecutablePath));

  if (!samePath(stagedServer, expectedServer)) {
    fail('staged server file must match server_executable beneath target/domios-package');
  }

  if (stagedClient && samePath(stagedServer, stagedClient)) {
    fail('staged server and client files must be distinct');
  }

  if (samePath(outputPath, stagedServer) || (stagedClient && samePath(outputPath, stagedClient))) {
    fail('manifest output must stay outside the server and client executables');
  }

  const [serverBytes, clientBytes] = await Promise.all([
    readStagedFile(stagedServer, 'staged server file'),
    stagedClient ? readStagedFile(stagedClient, 'staged client file') : Promise.resolve(null),
  ]);

  const identity = clientIdentity ?? { kind: 'sha256', sha256: digest(clientBytes) };

  const manifest = validateManifest({
    schema: SERVER_COMPONENT_SCHEMA,
    server_build: serverBuild,
    server_executable: serverExecutablePath,
    server_sha256: digest(serverBytes),
    client_identity: identity,
    api_version: apiVersion,
    bootstrap_protocol: bootstrapProtocol,
  });

  await mkdir(path.dirname(outputPath), { recursive: true });
  const temporaryPath = `${outputPath}.${randomUUID()}.tmp`;
  try {
    await writeFile(temporaryPath, `${JSON.stringify(manifest, null, 2)}\n`, { flag: 'wx' });
    await rename(temporaryPath, outputPath);
  } catch (error) {
    await rm(temporaryPath, { force: true });
    throw error;
  }

  return outputPath;
}

function parseArguments(args) {
  const allowed = new Set([
    '--server-file',
    '--client-file',
    '--server-build',
    '--server-executable',
    '--api-version',
    '--bootstrap-protocol',
  ]);
  const values = new Map();

  for (let index = 0; index < args.length; index += 2) {
    const name = args[index];
    const value = args[index + 1];
    if (!allowed.has(name) || value === undefined || value.startsWith('--')) {
      fail(`invalid or incomplete argument: ${name ?? ''}`);
    }
    if (values.has(name)) fail(`duplicate argument: ${name}`);
    values.set(name, value);
  }

  for (const name of allowed) {
    if (!values.has(name)) fail(`missing required argument: ${name}`);
  }

  return {
    serverFile: values.get('--server-file'),
    clientFile: values.get('--client-file'),
    serverBuild: values.get('--server-build'),
    serverExecutable: values.get('--server-executable'),
    apiVersion: parseVersion(values.get('--api-version'), '--api-version'),
    bootstrapProtocol: parseVersion(values.get('--bootstrap-protocol'), '--bootstrap-protocol'),
  };
}

async function main() {
  try {
    const outputPath = await createManifest(parseArguments(process.argv.slice(2)));
    process.stdout.write(`${MANIFEST_RELATIVE_PATH}\n`);
    return outputPath;
  } catch (error) {
    await rm(path.resolve(process.cwd(), MANIFEST_RELATIVE_PATH), { force: true });
    throw error;
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch(error => {
    process.stderr.write(`server component manifest refused: ${error.message}\n`);
    process.exitCode = 1;
  });
}
