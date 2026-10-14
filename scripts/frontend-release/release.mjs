import { readFile, writeFile, appendFile, mkdir, cp } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { deterministicArchive } from './archive.mjs';
import { canonicalManifest, createManifest, validateTag, checksum } from './manifest.mjs';
import { fixtureSign, productionSign, verifySignature } from './sign.mjs';
import { publishRelease } from './publish.mjs';

const fixtureRoot = fileURLToPath(new URL('./fixtures/', import.meta.url));
export async function prepare({ contract, tag, dist, output, dryRun, sourceSha }) {
  validateTag(tag, contract);
  const archive = await deterministicArchive(dist);
  const manifest = createManifest(tag, contract, archive);
  const canonical = canonicalManifest(manifest);
  const publicKey = dryRun ? await readFile(path.join(fixtureRoot, 'frontend-test.pub'), 'utf8') : contract.publicKey;
  if (!dryRun && (!publicKey || publicKey.trim().split('\n').at(-1) === (await readFile(path.join(fixtureRoot, 'frontend-test.pub'), 'utf8')).trim().split('\n').at(-1))) throw new Error('Production frontend identity is unset or fixture-only');
  const signature = dryRun ? fixtureSign(canonical, await readFile(path.join(fixtureRoot, 'frontend-test.pem'), 'utf8'), publicKey)
    : await productionSign(canonical, publicKey, process.env.FRONTEND_SIGNING_PRIVATE_KEY);
  verifySignature(canonical, signature, publicKey);
  await mkdir(output, { recursive: true });
  await cp(dist, path.join(output, 'dist'), { recursive: true });
  await writeFile(path.join(output, `${tag}.tar.gz`), archive);
  await writeFile(path.join(output, 'frontend-manifest.json'), canonical);
  await writeFile(path.join(output, 'frontend-manifest.json.minisig'), signature);
  await writeFile(path.join(output, 'frontend-verification.pub'), publicKey);
  await writeFile(path.join(output, 'provenance.json'), JSON.stringify({ contractVersion: contract.contractVersion, sourceSha, tag, archiveSha256: checksum(archive), minApiVersion: manifest.minApiVersion, maxApiVersion: manifest.maxApiVersion, fixtureSigning: dryRun, node: process.version }));
  return manifest;
}
function githubRemote(repository, output) {
  // Asset read-back returns the whole archive on stdout; the 1 MiB default buffer is too small.
  const gh = args => execFileSync('gh', args, { stdio: ['ignore', 'pipe', 'pipe'], timeout: 60000, maxBuffer: 256 * 1024 * 1024 });
  const asset = name => path.join(output, name);
  return {
    createDraft: async tag => {
      const releases = JSON.parse(gh(['api', `repos/${repository}/releases?per_page=100`]));
      const existing = releases.find(item => item.tag_name === tag);
      if (existing && !existing.draft) return true;
      if (!existing) gh(['release', 'create', tag, '--repo', repository, '--draft', '--latest=false', '--title', tag, '--notes', 'Independent signed frontend release']);
      execFileSync(process.execPath, ['scripts/release-clear-wedged-assets.mjs', repository, tag, 'frontend'], { stdio: 'inherit' });
      return false;
    },
    upload: async (tag, name) => { gh(['release', 'upload', tag, asset(name), '--clobber', '--repo', repository]); },
    download: async (tag, name) => {
      // The tags endpoint hides drafts, so the draft is read from the release list.
      const releases = JSON.parse(gh(['api', `repos/${repository}/releases?per_page=100`]));
      const release = releases.find(item => item.tag_name === tag);
      if (!release) throw new Error('Versioned release missing');
      const item = release.assets.find(item => item.name === name);
      if (!item) throw new Error('Versioned upload missing');
      return gh(['api', `repos/${repository}/releases/assets/${item.id}`, '-H', 'Accept: application/octet-stream']);
    },
    publish: async tag => { gh(['release', 'edit', tag, '--repo', repository, '--draft=false', '--latest=false']); },
    discover: async () => {
      gh(['release', 'view', 'frontend-latest', '--repo', repository]);
      gh(['release', 'upload', 'frontend-latest', asset('frontend-manifest.json'), asset('frontend-manifest.json.minisig'), '--clobber', '--repo', repository]);
    },
  };
}
export function isDryRun(event, requested) {
  return event === 'pull_request' || requested === 'true';
}
async function main() {
  const [command, tag, output = 'frontend-release-output'] = process.argv.slice(2);
  const contract = JSON.parse(await readFile('frontend-release-contract.json', 'utf8'));
  if (command === 'resolve-pr-tag') {
    const committedTag = `frontend-v${contract.version}`;
    validateTag(committedTag, contract);
    await appendFile(process.env.GITHUB_ENV, `FRONTEND_TAG=${committedTag}\n`);
    return;
  }
  validateTag(tag, contract);
  const dryRun = isDryRun(process.env.GITHUB_EVENT_NAME, process.env.FRONTEND_DRY_RUN);
  if (command === 'validate') return;
  if (command === 'prepare') { await prepare({ contract, tag, dist: 'dist', output, dryRun, sourceSha: process.env.GITHUB_SHA }); return; }
  if (command !== 'publish') throw new Error('Unknown frontend release operation');
  if (dryRun) throw new Error('Dry-run events cannot invoke publication');
  const manifest = JSON.parse(await readFile(path.join(output, 'frontend-manifest.json'), 'utf8'));
  if (manifest.releaseId !== tag || manifest.minApiVersion !== contract.minApiVersion || manifest.maxApiVersion !== contract.maxApiVersion || manifest.url !== createManifest(tag, contract, Buffer.alloc(0)).url) throw new Error('Artifact contract mismatch');
  await publishRelease({ manifest, archive: await readFile(path.join(output, `${tag}.tar.gz`)), signature: await readFile(path.join(output, 'frontend-manifest.json.minisig'), 'utf8'), publicKey: contract.publicKey, fixtureKey: await readFile(path.join(fixtureRoot, 'frontend-test.pub'), 'utf8'), dryRun }, githubRemote(contract.repository, output));
}
if (process.argv[1] === fileURLToPath(import.meta.url)) main().catch(error => { console.error(`Frontend release failed; no discovery update is authorized: ${error.code ?? ''} ${error.message}`.trim()); process.exitCode = 1; });
