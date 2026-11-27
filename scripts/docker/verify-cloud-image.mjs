#!/usr/bin/env node
// Caller: CI cloud-image.yml, release-cloud-image.yml; scripts/docker/cloud-image.test.mjs; skill github-release
// Verifies that a pushed GHCR cloud image digest is anonymously pullable, matches its expected
// revision/environment annotations and tags, and stays within the compressed size budget.
import { appendFile, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const MANIFEST_ACCEPT = [
  'application/vnd.oci.image.manifest.v1+json',
  'application/vnd.docker.distribution.manifest.v2+json',
].join(', ');
const DIGEST_RE = /^sha256:[a-f0-9]{64}$/;
const SHA_RE = /^[a-f0-9]{40}$/;

export function parseGhcrRepo(repo) {
  const match = /^ghcr\.io\/([a-z0-9][a-z0-9._-]*\/[a-z0-9][a-z0-9._-]*)$/.exec(repo ?? '');
  if (!match) throw new Error(`Expected lowercase ghcr.io/<owner>/<name> repository, got '${repo ?? ''}'`);
  return match[1];
}

export function computeCompressedBytes(manifest) {
  if (!manifest || typeof manifest !== 'object') throw new Error('Manifest payload must be a JSON object');
  if (!Array.isArray(manifest.layers) || manifest.layers.length === 0) {
    throw new Error('Image manifest has no layers (multi-arch index or malformed manifest is not supported here)');
  }
  const configSize = manifest.config?.size ?? 0;
  if (!Number.isSafeInteger(configSize) || configSize < 0) {
    throw new Error('Invalid manifest config size');
  }
  let total = configSize;
  for (const layer of manifest.layers) {
    if (!Number.isSafeInteger(layer?.size) || layer.size < 0) {
      throw new Error('Invalid manifest layer size');
    }
    total += layer.size;
  }
  return total;
}

export function verifyManifestContract({
  manifest,
  digest,
  headerDigest,
  expectedSha,
  expectedEnv,
  maxCompressedMiB,
}) {
  if (!DIGEST_RE.test(digest ?? '')) throw new Error(`Invalid digest '${digest ?? ''}'`);
  if (headerDigest && headerDigest !== digest) {
    throw new Error(`Registry returned digest '${headerDigest}', expected '${digest}'`);
  }
  if (!SHA_RE.test(expectedSha ?? '')) throw new Error(`Invalid expected source SHA '${expectedSha ?? ''}'`);
  if (expectedEnv !== 'qa' && expectedEnv !== 'release') {
    throw new Error(`Expected environment 'qa' or 'release', got '${expectedEnv ?? ''}'`);
  }
  if (!Number.isFinite(maxCompressedMiB) || maxCompressedMiB <= 0) {
    throw new Error('maxCompressedMiB must be a positive number');
  }
  const annotations = manifest.annotations ?? {};
  if (annotations['org.opencontainers.image.revision'] !== expectedSha) {
    throw new Error(
      `Manifest revision annotation '${annotations['org.opencontainers.image.revision'] ?? ''}' != expected '${expectedSha}'`,
    );
  }
  if (annotations['org.domios.environment'] !== expectedEnv) {
    throw new Error(
      `Manifest environment annotation '${annotations['org.domios.environment'] ?? ''}' != expected '${expectedEnv}'`,
    );
  }
  const compressedBytes = computeCompressedBytes(manifest);
  const maxBytes = Math.floor(maxCompressedMiB * 1024 * 1024);
  const compressedMiB = Number((compressedBytes / (1024 * 1024)).toFixed(1));
  if (compressedBytes > maxBytes) {
    throw new Error(
      `Compressed image size ${compressedMiB} MiB (${compressedBytes} bytes) exceeds budget ${maxCompressedMiB} MiB (${maxBytes} bytes)`,
    );
  }
  return { compressedBytes, compressedMiB };
}

export async function verifyCloudImage({
  repo,
  digest,
  expectedSha,
  expectedEnv,
  tags = [],
  maxCompressedMiB,
  fetchImpl = fetch,
}) {
  const repoPath = parseGhcrRepo(repo);
  if (!DIGEST_RE.test(digest ?? '')) throw new Error(`Invalid image digest '${digest ?? ''}'`);

  const tokenUrl = `https://ghcr.io/token?scope=repository:${repoPath}:pull`;
  const tokenResp = await fetchImpl(tokenUrl, { signal: AbortSignal.timeout(30_000) });
  if (!tokenResp.ok) {
    throw new Error(`Anonymous GHCR token request failed: HTTP ${tokenResp.status}`);
  }
  const { token } = await tokenResp.json();
  if (typeof token !== 'string' || !token) throw new Error('Anonymous GHCR token response was empty');

  const headers = { Authorization: `Bearer ${token}`, Accept: MANIFEST_ACCEPT };
  const digestUrl = `https://ghcr.io/v2/${repoPath}/manifests/${digest}`;
  const manifestResp = await fetchImpl(digestUrl, { headers, signal: AbortSignal.timeout(30_000) });
  if (!manifestResp.ok) {
    throw new Error(
      `Anonymous GHCR pull refused for ${repo}@${digest} (HTTP ${manifestResp.status}); ensure the package visibility is Public`,
    );
  }
  const headerDigest = manifestResp.headers.get('docker-content-digest');
  const manifest = await manifestResp.json();
  const { compressedBytes, compressedMiB } = verifyManifestContract({
    manifest,
    digest,
    headerDigest,
    expectedSha,
    expectedEnv,
    maxCompressedMiB,
  });

  for (const tag of tags) {
    const tagUrl = `https://ghcr.io/v2/${repoPath}/manifests/${encodeURIComponent(tag)}`;
    const tagResp = await fetchImpl(tagUrl, { headers, signal: AbortSignal.timeout(30_000) });
    if (!tagResp.ok) {
      throw new Error(`Anonymous GHCR pull refused for tag ${repo}:${tag} (HTTP ${tagResp.status})`);
    }
    const tagDigest = tagResp.headers.get('docker-content-digest');
    if (tagDigest !== digest) {
      throw new Error(`Tag ${repo}:${tag} resolved to '${tagDigest ?? ''}', expected '${digest}'`);
    }
  }

  return {
    imageRef: `${repo}@${digest}`,
    digest,
    tags,
    sourceSha: expectedSha,
    environment: expectedEnv,
    compressedBytes,
    compressedMiB,
    maxCompressedMiB,
    anonymousPull: 'ok',
  };
}

export function formatSummaryMarkdown(result) {
  const tagList = result.tags.length ? result.tags.map(tag => `\`${tag}\``).join(', ') : '(digest-only)';
  return [
    `### Cloud image verified (${result.environment})`,
    '',
    '| Field | Value |',
    '| --- | --- |',
    `| Pinned ref | \`${result.imageRef}\` |`,
    `| Digest | \`${result.digest}\` |`,
    `| Tags | ${tagList} |`,
    `| Source SHA | \`${result.sourceSha}\` |`,
    `| Anonymous pull | \`${result.anonymousPull}\` |`,
    `| Compressed size | ${result.compressedMiB} MiB (${result.compressedBytes} bytes; budget ${result.maxCompressedMiB} MiB) |`,
    '',
  ].join('\n');
}

function parseArgs(argv) {
  const args = { tags: [] };
  for (let i = 2; i < argv.length; i += 2) {
    const flag = argv[i];
    const value = argv[i + 1];
    if (!flag?.startsWith('--') || value === undefined) throw new Error(`Invalid argument: ${flag ?? ''}`);
    if (flag === '--tag') args.tags.push(value);
    else args[flag.slice(2)] = value;
  }
  for (const required of ['repo', 'digest', 'expected-sha', 'expected-env', 'max-compressed-mib']) {
    if (!args[required]) throw new Error(`Missing required flag --${required}`);
  }
  return args;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const args = parseArgs(process.argv);
  verifyCloudImage({
    repo: args.repo,
    digest: args.digest,
    expectedSha: args['expected-sha'],
    expectedEnv: args['expected-env'],
    tags: args.tags,
    maxCompressedMiB: Number(args['max-compressed-mib']),
  })
    .then(async result => {
      if (args['write-digest-file']) {
        await writeFile(args['write-digest-file'], `${result.imageRef}\n`, 'utf8');
      }
      if (args['summary-file']) {
        await appendFile(args['summary-file'], formatSummaryMarkdown(result), 'utf8');
      }
      console.log(JSON.stringify(result));
    })
    .catch(error => {
      console.error(`cloud image verification refused: ${error.message}`);
      process.exitCode = 1;
    });
}
