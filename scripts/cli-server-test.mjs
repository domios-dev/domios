import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { chmodSync, copyFileSync, cpSync, mkdirSync, readFileSync, realpathSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';
import { renderedCompilerErrors } from './collect-rust-warnings.mjs';

export function requiresCliServer(inventory) {
  return Object.values(inventory['rust-suites']).some(suite => suite['binary-name'] === 'isolated_server' &&
    Object.values(suite.testcases).some(test => test['filter-match'].status === 'matches'));
}

function buildArtifact(manifest, binary, env, extra = [], execute = execFileSync) {
  let output;
  try {
    output = execute('cargo', ['build', '--locked', '--manifest-path', manifest, '--bin', binary,
      ...extra, '--message-format=json'], { env, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024, stdio: ['ignore', 'pipe', 'inherit'] });
  } catch (error) {
    process.stderr.write(renderedCompilerErrors(error.stdout?.toString() || ''));
    throw error;
  }
  const messages = output.split(/\r?\n/).filter(Boolean).map(JSON.parse);
  const artifacts = messages.filter(row => row.reason === 'compiler-artifact' && row.target.name === binary && row.executable);
  if (!messages.some(row => row.reason === 'build-finished' && row.success) || artifacts.length !== 1) {
    throw new Error('Missing CLI integration artifact: ' + binary);
  }
  return realpathSync.native(artifacts[0].executable);
}

export function prepareCliServer({ cliTarget, serverTarget, env = process.env, execute } = {}) {
  if (!cliTarget || !serverTarget) throw new Error('CLI integration requires both target directories');
  const cli = buildArtifact('cli/Cargo.toml', 'codeterm', { ...env, CARGO_TARGET_DIR: cliTarget }, [], execute);
  const digest = createHash('sha256').update(readFileSync(cli)).digest('hex');
  const server = buildArtifact('src-tauri/Cargo.toml', 'domios-server',
    { ...env, CARGO_TARGET_DIR: serverTarget, CODETERM_SERVER_CLI_SHA256: digest }, ['-p', 'domios-server', '--no-default-features'], execute);
  const root = path.resolve(serverTarget, 'test-preparation', 'cli-server');
  const extension = process.platform === 'win32' ? '.exe' : '';
  const stagedServer = path.join(root, 'domios-server', 'domios-server' + extension);
  for (const [source, destination] of [[server, stagedServer], [cli, path.join(root, 'cli', 'codeterm' + extension)]]) {
    mkdirSync(path.dirname(destination), { recursive: true });
    copyFileSync(source, destination);
    chmodSync(destination, 0o755);
  }
  if (process.platform === 'win32') cpSync('src-tauri/conpty', path.join(root, 'conpty'), { recursive: true });
  return stagedServer;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  if (process.argv[2] === 'selected') {
    process.stdout.write(String(requiresCliServer(JSON.parse(readFileSync(process.argv[3], 'utf8')))) + '\n');
  } else process.stdout.write(prepareCliServer({ cliTarget: process.argv[2], serverTarget: process.argv[3] }) + '\n');
}
