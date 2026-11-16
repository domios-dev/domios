import { readFile, writeFile } from 'node:fs/promises';
import { isDeepStrictEqual } from 'node:util';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { publishConcurrently } from './release-promote.mjs';
import { PLATFORMS } from './release-dry-run-manifest.mjs';
import { loadPublicKey, verifyVersionBinding } from './verify-updater-manifest.mjs';

export function qualifiedMetadata(receipt, tag, sourceSha, runId) {
  if (receipt.schema !== 1 || receipt.mode !== 'local_dry_run' || receipt.tag !== tag ||
      receipt.source_sha !== sourceSha || String(receipt.run_id) !== String(runId) ||
      !isDeepStrictEqual(receipt.platforms, PLATFORMS) || receipt.evidence?.length !== PLATFORMS.length) {
    throw new Error('Foreign qualified release metadata');
  }
  const files = new Map();
  for (const platform of PLATFORMS) {
    const rows = receipt.evidence.filter(entry => entry.platform === platform);
    if (rows.length !== 1 || rows[0].source_sha !== sourceSha || rows[0].tag !== tag) throw new Error('Foreign platform metadata');
    for (const file of rows[0].files.filter(file => file.name !== 'dry-run-evidence.json')) {
      if (files.has(file.name) || !/^[A-Za-z0-9_.-]+$/.test(file.name) || file.name.includes('..') ||
          !/^[a-f0-9]{64}$/.test(file.sha256) || !Number.isSafeInteger(file.size) || file.size < 1) throw new Error('Invalid qualified asset metadata');
      files.set(file.name, file);
    }
  }
  return files;
}

export async function verifyServedMetadata({ receipt, tag, sourceSha, runId, repository, request = fetch, key }) {
  const files = qualifiedMetadata(receipt, tag, sourceSha, runId);
  const records = [];
  for (const name of ['domios-latest.json', 'domios-latest-deb.json']) {
    const url = `https://github.com/${repository}/releases/download/${tag}/${name}?cb=${Date.now()}`;
    const response = await request(url, { signal: AbortSignal.timeout(60_000) });
    if (response.status !== 200) throw new Error(`Manifest HTTP ${response.status}: ${url}`);
    const manifest = await response.json();
    const expected = receipt.manifests?.[name];
    if (!expected || !isDeepStrictEqual(manifest, expected) || manifest.version !== tag.slice(1)) throw new Error('Served manifest differs from qualification');
    const platforms = name.includes('-deb.') ? ['linux-aarch64', 'linux-x86_64'] : PLATFORMS;
    if (!isDeepStrictEqual(Object.keys(manifest.platforms).sort(), [...platforms].sort())) throw new Error('Incomplete qualified manifest');
    for (const entry of Object.values(manifest.platforms)) {
      const sourceUrl = new URL(entry.url);
      const file = files.get(path.posix.basename(sourceUrl.pathname));
      if (!file || entry.url !== `https://github.com/domios-dev/domios/releases/download/${tag}/${file.name}` ||
          entry.sha256 !== file.sha256 || entry.size !== file.size) throw new Error('Manifest asset differs from qualified digest or URL');
      verifyVersionBinding(entry.signature, manifest.version, key.keyId, key.publicKey);
    }
    records.push({ verified: true, mode: 'qualified_metadata', version: manifest.version, cache_busted: true,
      manifest: url, platforms, failures: [], qualification: { source_sha: sourceSha, run_id: String(runId) },
      verified_at: new Date().toISOString() });
  }
  await publishConcurrently([...files.values()].filter(file => repository === 'domios-dev/domios' || !/^updater-.*\.json$/.test(file.name)), async file => {
    const url = `https://github.com/${repository}/releases/download/${tag}/${file.name}`;
    const response = await request(url, { method: 'HEAD', redirect: 'follow', signal: AbortSignal.timeout(60_000) });
    if (response.status !== 200 || Number(response.headers.get('content-length')) !== file.size) throw new Error(`Served asset status/size mismatch: ${file.name}`);
  });
  return records;
}

async function main([verb, evidence, tag, sourceSha, runId, repository, output]) {
  const receipt = JSON.parse(await readFile(evidence, 'utf8'));
  if (verb === 'prepare') {
    qualifiedMetadata(receipt, tag, sourceSha, runId);
    for (const name of ['domios-latest.json', 'domios-latest-deb.json']) {
      if (!receipt.manifests?.[name]) throw new Error('Missing qualified manifest');
      await writeFile(path.join(output, name), JSON.stringify(receipt.manifests[name]));
    }
    return;
  }
  if (verb !== 'verify') throw new Error('Unknown served metadata operation');
  const records = await verifyServedMetadata({ receipt, tag, sourceSha, runId, repository, key: await loadPublicKey({}) });
  for (const [index, name] of ['manifest-verification.json', 'deb-manifest-verification.json'].entries()) {
    await writeFile(path.join(output, name), JSON.stringify(records[index], null, 2) + '\n');
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
