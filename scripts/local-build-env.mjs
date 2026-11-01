import fs from 'node:fs';
import path from 'node:path';
import { parseEnv } from 'node:util';
import { spawn, spawnSync } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const { gitOutput, gitExecutable } = createRequire(import.meta.url)('./build-identity.cjs');
export const buildSecretNames = {
  DOMIOS_FIREBASE_API_KEY: 'DOMIOS_FIREBASE_API_KEY',
  DOMIOS_FIREBASE_PROJECT_ID: 'DOMIOS_FIREBASE_PROJECT_ID',
  DOMIOS_FIREBASE_APP_ID: 'DOMIOS_FIREBASE_APP_ID',
  DOMIOS_ACCOUNT_REDEEM_URL: 'DOMIOS_ACCOUNT_REDEEM_URL',
  DOMIOS_QA_PROJECT_ID: 'DOMIOS_QA_PROJECT_ID',
  DOMIOS_QA_FIREBASE_API_KEY: 'DOMIOS_QA_FIREBASE_API_KEY',
  DOMIOS_QA_FIREBASE_APP_ID: 'DOMIOS_QA_FIREBASE_APP_ID',
  DOMIOS_BILLING_FUNCTIONS_URL: 'DOMIOS_BILLING_FUNCTIONS_URL',
  DOMIOS_FIREBASE_AUTH_DOMAIN: 'DOMIOS_FIREBASE_AUTH_DOMAIN',
  DOMIOS_SCCACHE_S3_ACCESS_KEY: 'domios-sccache-s3-access-key',
  DOMIOS_SCCACHE_S3_SECRET_KEY: 'domios-sccache-s3-secret-key',
  DOMIOS_CI_STORE_ENDPOINT: 'domios-ci-store-endpoint',
};
const firebaseKeys = Object.keys(buildSecretNames).slice(0, 8);

export function localBuildEnvPath(cwd = process.cwd()) {
  const common = gitOutput(['rev-parse', '--path-format=absolute', '--git-common-dir'], { cwd });
  const root = path.dirname(common);
  const file = path.join(root, '.env');
  if (spawnSync(gitExecutable(), ['check-ignore', '-q', '--', '.env'], { cwd: root }).status !== 0) {
    throw Error('Local build .env must be ignored by Git');
  }
  return file;
}

function readRegular(file) {
  const metadata = fs.lstatSync(file);
  if (!metadata.isFile() || metadata.isSymbolicLink()) throw Error('Local build .env must be a regular file');
  return fs.readFileSync(file, 'utf8');
}

export function readLocalBuildEnv(cwd = process.cwd()) {
  const file = localBuildEnvPath(cwd);
  let values;
  try { values = parseEnv(readRegular(file)); }
  catch (error) {
    if (error.code === 'ENOENT') throw Error('Missing local build .env; run ./build.sh dev-env once');
    throw error;
  }
  for (const key of firebaseKeys) {
    if (!values[key]?.trim()) throw Error(`Missing local build key ${key}; run ./build.sh dev-env once`);
  }
  return Object.fromEntries(Object.keys(buildSecretNames).filter(key => key in values).map(key => [key, values[key]]));
}

function vaultValue(cli, name, env) {
  return new Promise(resolve => {
    const child = spawn(cli, ['mem', 'secret', 'get', '--name', name], { env, stdio: ['ignore', 'pipe', 'pipe'] });
    let value = '';
    child.stdout.on('data', chunk => { value += chunk; });
    child.stderr.resume();
    child.once('error', () => resolve(''));
    child.once('close', code => resolve(code === 0 ? value.replace(/\n+$/, '') : ''));
  });
}

function restrictAccess(file, env) {
  if (process.platform !== 'win32') { fs.chmodSync(file, 0o600); return; }
  const command = '$identity=[System.Security.Principal.WindowsIdentity]::GetCurrent(); '
    + '$acl=[System.Security.AccessControl.FileSecurity]::new(); $acl.SetOwner($identity.User); '
    + '$acl.SetAccessRuleProtection($true,$false); '
    + '$acl.AddAccessRule([System.Security.AccessControl.FileSystemAccessRule]::new($identity.User,'
    + '[System.Security.AccessControl.FileSystemRights]::FullControl,[System.Security.AccessControl.AccessControlType]::Allow)); '
    + 'Set-Acl -LiteralPath $env:CODETERM_ENV_ACL_PATH -AclObject $acl';
  const args = ['-NoProfile', '-NonInteractive', '-Command', command];
  const options = { env: { ...env, CODETERM_ENV_ACL_PATH: file }, stdio: 'ignore' };
  let result = spawnSync('pwsh', args, options);
  if (result.error?.code === 'ENOENT') result = spawnSync('powershell.exe', args, options);
  if (result.status !== 0) throw Error('Cannot restrict local build .env to the current user');
}

export async function createLocalBuildEnv({ cwd = process.cwd(), env = process.env, readVault = vaultValue } = {}) {
  if (env.CI) throw Error('Local build .env creation is unavailable in CI');
  const file = localBuildEnvPath(cwd);
  const original = fs.existsSync(file) ? readRegular(file) : '';
  const previous = parseEnv(original);
  const entries = await Promise.all(Object.entries(buildSecretNames).map(async ([key, name]) =>
    [key, await readVault(env.CODETERM_ENV_VAULT_CLI || 'codeterm', name, env)]));
  for (const [key, value] of entries) {
    if (firebaseKeys.includes(key) && !value.trim()) throw Error(`Missing vault key ${key}`);
  }
  const retained = Object.entries(previous).filter(([key]) => !(key in buildSecretNames));
  const content = [...retained, ...entries].map(([key, value]) => `${key}=${JSON.stringify(value)}`).join('\n') + '\n';
  const parsed = parseEnv(content);
  for (const [key, value] of [...retained, ...entries]) {
    if (parsed[key] !== value) throw Error(`Unsupported local .env value encoding for ${key}`);
  }
  const candidate = `${file}.${randomUUID()}.pending`;
  fs.closeSync(fs.openSync(candidate, 'wx', 0o600));
  try {
    restrictAccess(candidate, env);
    if ((fs.existsSync(file) ? readRegular(file) : '') !== original) throw Error('Local build .env changed during retrieval');
    fs.writeFileSync(candidate, content);
    fs.renameSync(candidate, file);
  } finally { fs.rmSync(candidate, { force: true }); }
  return entries.filter(([, value]) => value.trim()).map(([key]) => key);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    if (process.argv[2] === 'create') {
      const keys = await createLocalBuildEnv();
      console.log(`Local ignored .env saved with user-only access; keys: ${keys.join(', ')}`);
    } else if (process.argv[2] === 'read') {
      for (const [key, value] of Object.entries(readLocalBuildEnv())) process.stdout.write(`${key}\0${value}\0`);
      process.stdout.write('__COMPLETE__\0true\0');
    } else throw Error('Expected local-build-env.mjs create|read');
  } catch (error) { console.error(error.message); process.exitCode = 2; }
}
