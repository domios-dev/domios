import { createHash, createPublicKey, sign, verify } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { mkdtemp, writeFile, readFile, rm } from 'node:fs/promises';
import os from 'node:os';
import path from 'node:path';

function decode(line, length) {
  if (typeof line !== 'string' || !/^[A-Za-z0-9+/]+={0,2}$/.test(line)) throw new Error('Invalid minisign base64');
  const bytes = Buffer.from(line, 'base64');
  if (bytes.length !== length || bytes.toString('base64') !== line) throw new Error('Invalid minisign length');
  return bytes;
}
export function verifySignature(bytes, signature, publicKey) {
  const lines = signature.trimEnd().split('\n');
  if (lines.length !== 4 || !lines[0].startsWith('untrusted comment: ') || !lines[2].startsWith('trusted comment: ')) throw new Error('Invalid minisign format');
  const key = decode(publicKey.trim().split('\n').at(-1), 42);
  const packet = decode(lines[1], 74);
  if (key.subarray(0, 2).toString() !== 'Ed' || packet.subarray(0, 2).toString() !== 'ED' || !key.subarray(2, 10).equals(packet.subarray(2, 10))) throw new Error('Wrong frontend signing identity or mode');
  const nativeKey = createPublicKey({ key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]), format: 'der', type: 'spki' });
  const digest = createHash('blake2b512').update(bytes).digest();
  const sig = packet.subarray(10);
  const comment = Buffer.from(lines[2].slice('trusted comment: '.length));
  if (!verify(null, digest, nativeKey, sig) || !verify(null, Buffer.concat([sig, comment]), nativeKey, decode(lines[3], 64))) throw new Error('Frontend signature verification failed');
  return true;
}
export function fixtureSign(bytes, privateKey, publicKey) {
  const key = decode(publicKey.trim().split('\n').at(-1), 42);
  const sig = sign(null, createHash('blake2b512').update(bytes).digest(), privateKey);
  const comment = 'frontend fixture contract 1';
  const packet = Buffer.concat([Buffer.from('ED'), key.subarray(2, 10), sig]);
  return `untrusted comment: frontend TEST ONLY\n${packet.toString('base64')}\ntrusted comment: ${comment}\n${sign(null, Buffer.concat([sig, Buffer.from(comment)]), privateKey).toString('base64')}\n`;
}
export async function productionSign(bytes, publicKey, secret) {
  if (!secret || !publicKey) throw new Error('Dedicated frontend signing identity is not provisioned');
  const directory = await mkdtemp(path.join(os.tmpdir(), 'frontend-sign-'));
  try {
    await writeFile(path.join(directory, 'key'), secret, { mode: 0o600 });
    await writeFile(path.join(directory, 'manifest'), bytes);
    // Capture subprocess output so signing failures cannot leak secret material to CI logs.
    try { execFileSync('minisign', ['-S', '-W', '-s', path.join(directory, 'key'), '-m', path.join(directory, 'manifest'), '-t', 'frontend release contract 1'], { stdio: ['ignore', 'pipe', 'pipe'], timeout: 30000 }); }
    catch { throw new Error('Frontend signing failed'); }
    const signature = await readFile(path.join(directory, 'manifest.minisig'), 'utf8');
    verifySignature(bytes, signature, publicKey);
    return signature;
  } finally { await rm(directory, { recursive: true, force: true }); }
}
