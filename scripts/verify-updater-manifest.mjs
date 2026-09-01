#!/usr/bin/env node
// Verify that each platform signature in an updater manifest actually validates
// against the bytes served at that platform's URL. HTTP 200 proves nothing: a
// rebuilt artifact under a kept signature 200s and then fails in every client.
//
// Usage:
//   verify-updater-manifest.mjs --manifest <file|url> [--pubkey <base64|@file>]
//                               [--local <dir>] [--platform <key>]
//                               [--require-platform <key>]...
//                               [--require-version <x.y.z>] [--evidence <file>]
// The pubkey defaults to src-tauri/tauri.conf.json's updater key.
// --local resolves each URL to <dir>/<basename> instead of downloading.
// --require-version pins the manifest to one release.
// --evidence writes the record that authorises closing public release issues.

import { createHash, createPublicKey, verify as edVerify } from 'node:crypto';
import { readFile, writeFile, rename } from 'node:fs/promises';
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
  const args = {
    manifest: null,
    pubkey: null,
    local: null,
    platform: null,
    requiredPlatforms: [],
    requireVersion: null,
    evidence: null,
  };
  for (let i = 2; i < argv.length; i++) {
    const flag = argv[i];
    if (flag === '--manifest') args.manifest = argv[++i];
    else if (flag === '--pubkey') args.pubkey = argv[++i];
    else if (flag === '--local') args.local = argv[++i];
    else if (flag === '--platform') args.platform = argv[++i];
    else if (flag === '--require-platform') args.requiredPlatforms.push(argv[++i]);
    else if (flag === '--require-version') args.requireVersion = argv[++i];
    else if (flag === '--evidence') args.evidence = argv[++i];
    else throw new Error(`Unknown argument: ${flag}`);
  }
  if (!args.manifest) {
    throw new Error(
      'Usage: --manifest <file|url> [--pubkey <base64|@file>] [--local <dir>] '
      + '[--platform <key>] [--require-platform <key>]... '
      + '[--require-version <x.y.z>] [--evidence <file>]',
    );
  }
  return args;
}

const PLATFORM_ARTIFACTS = new Map([
  ['darwin-aarch64', 'CodeTerm_aarch64.app.tar.gz'],
  ['darwin-x86_64', 'CodeTerm_x64.app.tar.gz'],
]);

// The one origin the shipped updater reads from. A version segment anywhere in
// an arbitrary path is not evidence the bytes came from the public release.
const PUBLIC_RELEASE_ASSET = /^\/rollacode\/codeterm-releases\/releases\/download\/([^/]+)\/[^/]+$/;

// Artifacts carried over from a previous tag are internally consistent and
// describe a build this release never produced, so the serving tag is checked.
function servedFromRelease(url, version) {
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return false;
  }
  if (parsed.protocol !== 'https:' || parsed.host !== 'github.com') return false;
  const asset = PUBLIC_RELEASE_ASSET.exec(parsed.pathname);
  // The tag carries any rc/nightly suffix, and build-updater-fragment.py derives
  // the version by stripping its leading 'v', so both forms are the same release.
  return asset !== null && (asset[1] === `v${version}` || asset[1] === version);
}

function validateReleaseScope(manifest, platforms, version) {
  if (manifest.version !== version) {
    throw new Error(`manifest declares version ${manifest.version}, expected ${version}`);
  }
  const foreign = Object.entries(platforms)
    .filter(([, entry]) => !servedFromRelease(entry.url, version))
    .map(([name]) => name);
  if (foreign.length > 0) {
    throw new Error(`${foreign.join(', ')} is not served from the v${version} release`);
  }
}

function validatePlatformContract(platforms, requiredPlatforms) {
  for (const name of requiredPlatforms) {
    if (!Object.hasOwn(platforms, name)) {
      throw new Error(`manifest is missing required platform '${name}'`);
    }
  }
  for (const [name, expectedArtifact] of PLATFORM_ARTIFACTS) {
    const entry = platforms[name];
    if (!entry) continue;
    const actualArtifact = path.basename(new URL(entry.url).pathname);
    if (actualArtifact !== expectedArtifact) {
      throw new Error(
        `${name} must reference ${expectedArtifact}, found ${actualArtifact || '<empty>'}`,
      );
    }
  }
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

// Rename so a reader never observes a half-written record and mistakes a
// truncated file for a verification that did not happen.
async function writeEvidence(target, record) {
  if (!target) return;
  const staging = `${target}.tmp`;
  await writeFile(staging, `${JSON.stringify(record, null, 2)}\n`);
  await rename(staging, target);
}

// Default to the key the shipped app verifies with, so a call site cannot check
// the manifest against a key no client uses.
async function loadPublicKey(args) {
  const raw = args.pubkey
    ? (args.pubkey.startsWith('@') ? await readFile(args.pubkey.slice(1), 'utf8') : args.pubkey)
    : JSON.parse(await readFile(TAURI_CONF, 'utf8')).plugins.updater.pubkey;
  const { keyId, key } = decodePublicKey(raw);
  return {
    keyId,
    publicKey: createPublicKey({
      key: Buffer.concat([SPKI_ED25519_PREFIX, key]),
      format: 'der',
      type: 'spki',
    }),
  };
}

async function verifyInto(record, args) {
  const { keyId, publicKey } = await loadPublicKey(args);

  const manifest = JSON.parse((await readSource(args.manifest)).toString('utf8'));
  record.version = manifest.version ?? null;
  const declaredPlatforms = manifest.platforms ?? {};
  validatePlatformContract(declaredPlatforms, args.requiredPlatforms);
  if (args.requireVersion) {
    validateReleaseScope(manifest, declaredPlatforms, args.requireVersion);
  }

  const platforms = Object.entries(declaredPlatforms).filter(
    ([name]) => !args.platform || name === args.platform,
  );
  if (platforms.length === 0) throw new Error('manifest declares no matching platform');

  console.log(`manifest version ${manifest.version} (pub_date ${manifest.pub_date})`);
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
      if (ok) record.platforms.push(name);
      else record.failures.push(`${name}: signature does not match the served bytes`);
    } catch (error) {
      console.log(`  FAIL ${name}  ${error.message}`);
      record.failures.push(`${name}: ${error.message}`);
    }
  }
}

async function main() {
  const args = parseArgs(process.argv);
  const record = {
    verified: false,
    version: null,
    cache_busted: /^https?:\/\//.test(args.manifest)
      && new URL(args.manifest).searchParams.has('cb'),
    manifest: args.manifest,
    platforms: [],
    failures: [],
    verified_at: null,
  };

  // Every exit path leaves a truthful record: a verification that ran and failed
  // must be distinguishable from one that never ran at all.
  try {
    await verifyInto(record, args);
  } catch (error) {
    record.failures.push(error.message);
    throw error;
  } finally {
    record.verified = record.failures.length === 0 && record.platforms.length > 0;
    record.verified_at = new Date().toISOString();
    await writeEvidence(args.evidence, record);
  }

  if (record.failures.length > 0) {
    console.error(`\nupdater manifest is not installable:\n  ${record.failures.join('\n  ')}`);
    process.exitCode = 1;
    return;
  }
  console.log(`\nall ${record.platforms.length} platform signature(s) verify against the served bytes`);
}

await main();
