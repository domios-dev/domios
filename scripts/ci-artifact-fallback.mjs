// Caller: scripts ci-artifact.sh, ci-artifact.test.mjs
// Reads run-artifact copies of spooled CI evidence; with a store listing, resolves each object against the store copy.
import fs from 'node:fs';
import path from 'node:path';
import {pathToFileURL} from 'node:url';

// Store LastModified, GitHub created_at and runner mtimes come from different clocks; closer copies are not ordered.
export const COPY_ORDER_SKEW_MS = 5 * 60_000;

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

const xmlText = text => text.replace(/&(lt|gt|quot|apos|amp);/g, (_, name) => ({lt: '<', gt: '>', quot: '"', apos: "'", amp: '&'})[name]);

export function parseStoreListing(xml) {
  if (/<IsTruncated>true<\/IsTruncated>/.test(xml)) throw Error('ci-artifact: store listing is truncated');
  return [...xml.matchAll(/<Contents>([\s\S]*?)<\/Contents>/g)].map(([, body]) => {
    const key = body.match(/<Key>([^<]*)<\/Key>/)?.[1];
    const modified = Date.parse(body.match(/<LastModified>([^<]*)<\/LastModified>/)?.[1]);
    if (!key || !Number.isFinite(modified)) throw Error('ci-artifact: store listing entry lacks a key or LastModified');
    return {key: xmlText(key), modified};
  });
}

// Each copy carries the clock its time comes from; the newest wins unless a copy on another clock is within the skew.
export function latestCopy(copies, artifacts, objectKey) {
  if (copies.length === 1) return copies[0];
  const dated = copies.map(copy => {
    if (copy.source === 'store' || copy.clock === 'runner') return copy;
    const matching = artifacts.filter(artifact => artifact.name === copy.artifact && !artifact.expired);
    if (matching.length !== 1 || !Number.isSafeInteger(matching[0].id) || !Number.isFinite(Date.parse(matching[0].created_at))) {
      throw Error(`Missing or ambiguous fallback artifact provenance: ${copy.artifact}`);
    }
    return {...copy, clock: 'github', modified: Date.parse(matching[0].created_at), id: matching[0].id};
  }).sort((a, b) => b.modified - a.modified || (b.id ?? 0) - (a.id ?? 0));
  const [newest] = dated;
  const rival = dated.find(copy => copy !== newest && copy.clock !== newest.clock && newest.modified - copy.modified < COPY_ORDER_SKEW_MS);
  if (rival) {
    throw Error(`ci-artifact: copy_order_ambiguous ${objectKey}: ${describe(newest)} and ${describe(rival)} are within ${COPY_ORDER_SKEW_MS / 1000}s`);
  }
  return newest;
}

const describe = copy => `${copy.source === 'store' ? 'private-store' : `run-artifact ${copy.artifact}`} (${new Date(copy.modified).toISOString()})`;

// Without a store listing (store down) only run artifacts are read; with one, spooled copies join the store objects.
// Returns the store keys the caller still has to fetch.
export async function materializeFallback(cache, key, destination, prefix, inventory = artifactInventory,
  {store = null, runnerTemp = process.env.RUNNER_TEMP, sources = process.env.CI_ARTIFACT_SOURCES} = {}) {
  const groups = new Map();
  const add = (objectKey, copy) => groups.set(objectKey, [...(groups.get(objectKey) || []), {source: 'artifact', clock: 'github', ...copy}]);
  const addSpooled = (artifact, source, extra = () => ({})) => {
    if (!fs.existsSync(source)) return;
    const files = prefix ? fs.readdirSync(source).map(name => path.join(source, name)) : [source];
    for (const file of files.filter(file => fs.statSync(file).isFile())) {
      add(prefix ? key + path.basename(file) : key, {artifact, file, ...extra(file)});
    }
  };
  if (store === null || fs.existsSync(cache)) {
    addSpooled('run-artifact', path.join(cache, key));
    for (const entry of fs.readdirSync(cache, {withFileTypes: true}).filter(entry => entry.isDirectory())) {
      addSpooled(entry.name, path.join(cache, entry.name, key));
    }
  }
  const spool = path.join(runnerTemp, 'ci-artifact-spool');
  if (fs.existsSync(path.join(spool, key))) {
    const name = fs.readFileSync(path.join(spool, '.name'), 'utf8').trim();
    // Not uploaded yet, so this job's own spool is dated by the runner clock.
    addSpooled(name, path.join(spool, key), file => ({clock: 'runner', modified: fs.statSync(file).mtimeMs}));
  }
  for (const object of store ?? []) {
    if (prefix ? !object.key.startsWith(key) : object.key !== key) continue;
    groups.set(object.key, [...(groups.get(object.key) || []), {source: 'store', clock: 'store', key: object.key, modified: object.modified}]);
  }
  if (!groups.size) throw Error(`ci-artifact: ${key} is in ${store ? 'neither the private store nor any' : 'no'} run artifact`);
  const needsInventory = [...groups.values()].some(copies => copies.length > 1 && copies.some(copy => copy.clock === 'github'));
  const artifacts = needsInventory ? await inventory(key.split('/')[0]) : [];
  const selected = [...groups].map(([objectKey, copies]) => [objectKey, latestCopy(copies, artifacts, objectKey)]);
  const fetch = [];
  if (prefix) fs.mkdirSync(destination, {recursive: true});
  for (const [objectKey, copy] of selected) {
    if (copy.source === 'store') { fetch.push(objectKey); continue; }
    fs.copyFileSync(copy.file, prefix ? path.join(destination, path.basename(copy.file)) : destination);
    if (sources) fs.appendFileSync(sources, `${objectKey}\trun-artifact:${copy.artifact}/${objectKey}\n`);
    console.log(`ci-artifact: got ${objectKey} from ${copy.artifact}`);
  }
  return fetch;
}

// node ci-artifact-fallback.mjs <cache> <key> <destination> get|get-prefix [<store-listing.xml> <store-plan>]
if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  const [cache, key, destination, verb, listing, plan] = process.argv.slice(2);
  const store = listing ? parseStoreListing(fs.readFileSync(listing, 'utf8')) : null;
  materializeFallback(cache, key, destination, verb === 'get-prefix', artifactInventory, {store})
    .then(fetch => { if (plan) fs.writeFileSync(plan, fetch.map(objectKey => objectKey + '\n').join('')); })
    .catch(error => {console.error(error.message); process.exitCode = 1;});
}
