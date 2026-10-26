// Caller: CI release-credentials.yml (hosted-signing); skill github-release
// Signs a small file with the updater key through the Tauri signer and verifies it against the shipped public key,
// so a runner proves it can sign updates without building anything.
import { execFileSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { verifySignature } from './frontend-release/sign.mjs';

const directory = mkdtempSync(path.join(process.env.RUNNER_TEMP || os.tmpdir(), 'updater-probe-'));
try {
  const file = path.join(directory, 'probe.bin');
  const bytes = randomBytes(4096);
  writeFileSync(file, bytes);
  // pnpm is a .cmd shim on Windows, which Node only starts through a shell.
  execFileSync('pnpm', ['tauri', 'signer', 'sign', file], { stdio: ['ignore', 'pipe', 'pipe'], shell: process.platform === 'win32' });
  const signature = Buffer.from(readFileSync(`${file}.sig`, 'utf8').trim(), 'base64').toString('utf8');
  const config = JSON.parse(readFileSync('src-tauri/tauri.conf.json', 'utf8'));
  const publicKey = Buffer.from(config.plugins.updater.pubkey, 'base64').toString('utf8');
  verifySignature(bytes, signature, publicKey);
  console.log(`updater signing probe PASS on ${process.platform}-${process.arch}: signature verifies against tauri.conf.json`);
} finally {
  rmSync(directory, { recursive: true, force: true });
}
