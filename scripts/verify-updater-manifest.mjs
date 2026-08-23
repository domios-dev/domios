#!/usr/bin/env node
// Verify that each platform signature in an updater manifest actually validates
// against the bytes served at that platform's URL. HTTP 200 proves nothing: a
// rebuilt artifact under a kept signature 200s and then fails in every client.
//
// Usage:
//   verify-updater-manifest.mjs --manifest <file|url> [--pubkey <base64|@file>]
//                               [--local <dir>] [--platform <key>]
// The pubkey defaults to src-tauri/tauri.conf.json's updater key.
// --local resolves each URL to <dir>/<basename> instead of downloading.

import { createHash, createPublicKey, verify as edVerify } from 'node:crypto';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const TAURI_CONF = path.join(
  path.dirname(fileURLToPath(import.meta.url)),
  '..',
  'src-tauri',
  'tauri.conf.json',
);

const SPKI_ED25519_PREFIX = Buffer.from('302a300506032b6570032100', 'hex');

function parseArgs(argv) {
  const args = { manifest: null, pubkey: null, local: null, platform: null };
  for (let i = 2; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--manifest') args.manifest = argv[++i];
    else if (flag === '--pubkey') args.pubkey = argv[++i];
    else if (flag === '--local') args.local = argv[++i];
    else if (flag === '--platform') args.platform = argv[++i];
    else throw new Error(`Unknown argument: ${flag}`);
  }
  if (!args.manifest) {
    throw new Error('Usage: --manifest <file|url> [--pubkey <base64|@file>] [--local <dir>] [--platform <key>]');
  }
  return args;
}

async function readSource(source) {
  if (/^https?:\/\//.test(source)) {
    const response = await fetch(source, { redirect: 'follow' });
    if (!response.ok) throw new Error(`GET ${source} -> HTTP ${response.status}`);
    return Buffer.from(await response.arrayBuffer());
  }
  return readFile(source);
}

function decodePublicKey(raw) {
  const decoded = Buffer.from(raw.trim(), 'base64').toString('utf8');
  const payload = decoded.split('\n').filter((line) => line && !line.startsWith('untrusted comment:'));
  const bytes = Buffer.from(payload[payload.length - 1].trim(), 'base64');
  if (bytes.length !== 42) throw new Error(`public key payload is ${bytes.length} bytes, expected 42`);
  return { keyId: bytes.subarray(2, 10), key: bytes.subarray(10) };
}

function decodeSignature(field) {
  const text = Buffer.from(field.trim(), 'base64').toString('utf8');
  const lines = text.split('\n').filter((line) => line.trim() !== '');
  const body = lines.find((line) => !line.startsWith('untrusted comment:') && !line.startsWith('trusted comment:'));
  if (!body) throw new Error('signature carries no payload line');
  const bytes = Buffer.from(body.trim(), 'base64');
  if (bytes.length !== 74) throw new Error(`signature payload is ${bytes.length} bytes, expected 74`);
  return {
    algorithm: bytes.subarray(0, 2).toString('utf8'),
    keyId: bytes.subarray(2, 10),
    signature: bytes.subarray(10),
  };
}

function signedMessage(algorithm, content) {
  if (algorithm === 'ED') return createHash('blake2b512').update(content).digest();
  if (algorithm === 'Ed') return content;
  throw new Error(`unsupported signature algorithm '${algorithm}'`);
}

async function main() {
  const args = parseArgs(process.argv);
  // Default to the key the shipped app verifies with, so a call site cannot
  // check the manifest against a key no client uses.
  const pubkeyRaw = args.pubkey
    ? (args.pubkey.startsWith('@') ? await readFile(args.pubkey.slice(1), 'utf8') : args.pubkey)
    : JSON.parse(await readFile(TAURI_CONF, 'utf8')).plugins.updater.pubkey;
  const { keyId, key } = decodePublicKey(pubkeyRaw);
  const publicKey = createPublicKey({
    key: Buffer.concat([SPKI_ED25519_PREFIX, key]),
    format: 'der',
    type: 'spki',
  });

  const manifest = JSON.parse((await readSource(args.manifest)).toString('utf8'));
  const platforms = Object.entries(manifest.platforms ?? {}).filter(
    ([name]) => !args.platform || name === args.platform,
  );
  if (platforms.length === 0) throw new Error('manifest declares no matching platform');

  console.log(`manifest version ${manifest.version} (pub_date ${manifest.pub_date})`);
  const failures = [];
  for (const [name, entry] of platforms) {
    try {
      const content = args.local
        ? await readFile(path.join(args.local, path.basename(new URL(entry.url).pathname)))
        : await readSource(entry.url);
      const { algorithm, keyId: sigKeyId, signature } = decodeSignature(entry.signature ?? '');
      if (!sigKeyId.equals(keyId)) {
        throw new Error(`signed by key ${sigKeyId.toString('hex')}, manifest key is ${keyId.toString('hex')}`);
      }
      const ok = edVerify(null, signedMessage(algorithm, content), publicKey, signature);
      console.log(`  ${ok ? 'OK  ' : 'FAIL'} ${name}  ${content.length} bytes  ${entry.url}`);
      if (!ok) failures.push(`${name}: signature does not match the served bytes`);
    } catch (error) {
      console.log(`  FAIL ${name}  ${error.message}`);
      failures.push(`${name}: ${error.message}`);
    }
  }
  if (failures.length > 0) {
    console.error(`\nupdater manifest is not installable:\n  ${failures.join('\n  ')}`);
    process.exitCode = 1;
    return;
  }
  console.log(`\nall ${platforms.length} platform signature(s) verify against the served bytes`);
}

await main();
