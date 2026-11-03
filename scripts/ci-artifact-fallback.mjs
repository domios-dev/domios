import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';

async function artifactInventory(run) {
  const repository = process.env.GITHUB_REPOSITORY;
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository || '')) throw Error('Missing fallback artifact repository');
  const artifacts = [];
  for (let page = 1; page <= 20; page++) {
    const response = await fetch(`${process.env.GITHUB_API_URL || 'https://api.github.com'}/repos/${repository}/actions/runs/${run}/artifacts?per_page=100&page=${page}`, {
      headers: {Authorization: `Bearer ${process.env.GH_TOKEN || process.env.GITHUB_TOKEN || ''}`, Accept: 'application/vnd.github+json'},
      signal: AbortSignal.timeout(30_000),
    });
    if (!response.ok) throw Error(`Fallback artifact inventory failed: HTTP ${response.status}`);
    const batch = await response.json();
    artifacts.push(...batch.artifacts);
    if (artifacts.length >= batch.total_count) return artifacts;
    if (!batch.artifacts.length) break;
  }
  throw Error('Incomplete fallback artifact inventory');
}

export function latestArtifactCopy(copies, artifacts) {
  if (copies.length === 1) return copies[0];
  const ranked = copies.map(copy => {
    const matching = artifacts.filter(artifact => artifact.name === copy.artifact && !artifact.expired);
    if (matching.length !== 1 || !Number.isSafeInteger(matching[0].id) || !Number.isFinite(Date.parse(matching[0].created_at))) {
      throw Error(`Missing or ambiguous fallback artifact provenance: ${copy.artifact}`);
    }
    return {...copy, uploaded: Date.parse(matching[0].created_at), id: matching[0].id};
  }).sort((a, b) => b.uploaded - a.uploaded || b.id - a.id);
  return ranked[0];
}

export async function materializeFallback(cache, key, destination, prefix, inventory = artifactInventory) {
  const groups = new Map();
  const add = (artifact, source) => {
    if (!fs.existsSync(source)) return;
    const files = prefix ? fs.readdirSync(source).map(name => path.join(source, name)) : [source];
    for (const file of files.filter(file => fs.statSync(file).isFile())) {
      const objectKey = prefix ? key + path.basename(file) : key;
      const copies = groups.get(objectKey) || [];
      copies.push({artifact, file});
      groups.set(objectKey, copies);
    }
  };
  add('run-artifact', path.join(cache, key));
  for (const entry of fs.readdirSync(cache, {withFileTypes: true}).filter(entry => entry.isDirectory())) {
    add(entry.name, path.join(cache, entry.name, key));
  }
  const spool = path.join(process.env.RUNNER_TEMP, 'ci-artifact-spool');
  if (fs.existsSync(path.join(spool, key))) {
    const name = fs.readFileSync(path.join(spool, '.name'), 'utf8').trim();
    add(name, path.join(spool, key));
  }
  if (!groups.size) throw Error(`ci-artifact: ${key} is in no run artifact`);
  const artifacts = [...groups.values()].some(copies => copies.length > 1) ? await inventory(key.split('/')[0]) : [];
  const selected = [...groups].map(([objectKey, copies]) => [objectKey, latestArtifactCopy(copies, artifacts)]);
  if (prefix) fs.mkdirSync(destination, {recursive: true});
  for (const [objectKey, copy] of selected) {
    fs.copyFileSync(copy.file, prefix ? path.join(destination, path.basename(copy.file)) : destination);
    if (process.env.CI_ARTIFACT_SOURCES) fs.appendFileSync(process.env.CI_ARTIFACT_SOURCES, `${objectKey}\trun-artifact:${copy.artifact}/${objectKey}\n`);
    console.log(`ci-artifact: got ${objectKey} from ${copy.artifact}`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  materializeFallback(process.argv[2], process.argv[3], process.argv[4], process.argv[5] === 'get-prefix')
    .catch(error => {console.error(error.message); process.exitCode = 1;});
}
