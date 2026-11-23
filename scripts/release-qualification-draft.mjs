import { RELEASE_BRANCH, RELEASE_REF } from './release-version.mjs';
import { createReadStream } from 'node:fs';
import { stat } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ownsReleaseAsset } from './release-asset-scope.mjs';
import { sourceRepository } from './source-repository.mjs';
import { verifyAssetDigests, promotionPlan } from './release-asset-inventory.mjs';

const exec = promisify(execFile);
export async function gh(args, env = process.env) {
  const { stdout } = await exec('gh', args, { env, encoding: 'utf8', timeout: 30 * 60_000, maxBuffer: 8 * 1024 * 1024 });
  return stdout;
}

export function qualificationTag(tag, runId) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag) || !/^[1-9]\d*$/.test(String(runId))) throw new Error('Invalid qualification tag');
  return `qualification-${tag}-${runId}`;
}

export function releaseTitle(tag) {
  if (!/^v\d+\.\d+\.\d+$/.test(tag)) throw new Error('Invalid release tag');
  return `Domios ${tag.slice(1)}`;
}

const QUALIFICATION_MARKER = '<!-- qualification:';

function versionKey(version) {
  return version.split('.').map(part => part.padStart(8, '0')).join('.');
}

// A draft is ours only with a run marker; untagged-* placeholders carry the version in their title.
export function draftVersion(release) {
  if (!release?.draft || !release.body?.includes(QUALIFICATION_MARKER)) return null;
  const hidden = /^qualification-v(\d+\.\d+\.\d+)-[1-9]\d*$/.exec(release.tag_name ?? '');
  if (hidden) return hidden[1];
  if (!/^untagged-[0-9a-f]+$/.test(release.tag_name ?? '')) return null;
  return /^Domios v?(\d+\.\d+\.\d+)$/.exec(release.name ?? '')?.[1] ?? null;
}

export function supersededDrafts(releases, tag) {
  const published = versionKey(releaseTitle(tag).slice('Domios '.length));
  return releases.filter(release => {
    const version = draftVersion(release);
    return version !== null && versionKey(version) <= published;
  });
}

export async function pruneSupersededDrafts({ tag, repository, env, run = gh }) {
  const current = await readDraft(tag, repository, env, run);
  if (!current || current.isDraft || current.tagName !== tag) throw new Error(`${repository} ${tag} is not published`);
  const releases = (await run(['api', '--paginate', '--jq', '.[] | tojson',
    `repos/${repository}/releases?per_page=100`], env)).split('\n').filter(Boolean).map(JSON.parse);
  const removed = [];
  for (const old of supersededDrafts(releases, tag)) {
    try { await run(['api', '--method', 'DELETE', `repos/${repository}/releases/${old.id}`], env); }
    catch (error) { if (!/HTTP 404/.test(error.stderr ?? error.message)) throw error; }
    removed.push(old.tag_name);
  }
  return removed;
}

export function draftNotes(sourceSha, runId) {
  if (!/^[a-f0-9]{40}$/.test(sourceSha) || !/^[1-9]\d*$/.test(String(runId))) throw new Error('Invalid draft identity');
  return `<!-- qualification:${JSON.stringify({ source_sha: sourceSha, run_id: String(runId) })} -->`;
}

export function admitDraft(draft, tag, sourceSha, runId, repository = sourceRepository()) {
  if (!draft?.isDraft || ![tag, qualificationTag(tag, runId)].includes(draft.tagName) ||
      (repository === sourceRepository() && draft.targetCommitish !== sourceSha) ||
      !draft.body?.includes(draftNotes(sourceSha, runId))) throw new Error('Foreign qualification draft');
  return draft.databaseId;
}

export async function readDraft(tag, repository, env, run = gh) {
  try {
    return JSON.parse(await run(['release', 'view', tag, '--repo', repository, '--json',
      'databaseId,isDraft,tagName,targetCommitish,body'], env));
  } catch (error) {
    if (/release not found|HTTP 404/i.test(error.stderr ?? error.message)) return null;
    throw error;
  }
}

export async function prepareDraft({ tag, sourceSha, runId, repository, env, run = gh }) {
  const hiddenTag = qualificationTag(tag, runId);
  const old = await readDraft(hiddenTag, repository, env, run);
  if (old) {
    if (!old.isDraft) throw new Error('Cannot replace a published release');
    if (old.body?.includes(draftNotes(sourceSha, runId))) {
      admitDraft(old, tag, sourceSha, runId, repository);
      return;
    }
    throw new Error('Cannot replace a foreign run draft');
  }
  // A draft for a shipped version can never be promoted and would linger in Releases.
  const shipped = await readDraft(tag, repository, env, run);
  if (shipped && !shipped.isDraft) throw new Error(`${repository} ${tag} is already published; bump the version`);
  await run(['release', 'create', hiddenTag, '--repo', repository,
    ...(repository === sourceRepository() ? ['--target', sourceSha] : []), '--draft',
    '--title', releaseTitle(tag), '--notes', draftNotes(sourceSha, runId)], env);
}

export async function uploadAsset({ repository, id, name, file, env }) {
  const response = await fetch(`https://uploads.github.com/repos/${repository}/releases/${id}/assets?name=${encodeURIComponent(name)}`, {
    method: 'POST', redirect: 'error', signal: AbortSignal.timeout(30 * 60_000), duplex: 'half',
    headers: { Authorization: `Bearer ${env.GH_TOKEN}`, 'Content-Type': 'application/octet-stream',
      'Content-Length': String((await stat(file)).size), Accept: 'application/vnd.github+json' },
    body: createReadStream(file),
  });
  if (!response.ok) throw new Error(`Draft asset upload ${name}: HTTP ${response.status}`);
  return response.json();
}

export async function uploadDraft({ tag, sourceSha, runId, directory, receipt, repository, env, run = gh, upload = uploadAsset }) {
  const draft = await readDraft(qualificationTag(tag, runId), repository, env, run);
  const id = admitDraft(draft, tag, sourceSha, runId, repository);
  const names = repository === sourceRepository() ? promotionPlan(receipt).source : promotionPlan(receipt).mirror;
  const assets = (await run(['api', '--paginate', '--jq', '.[] | tojson',
    `repos/${repository}/releases/${id}/assets?per_page=100`], env)).split('\n').filter(Boolean).map(JSON.parse);
  for (const asset of assets.filter(asset => names.includes(asset.name) || asset.name === `qualification-${receipt.platform}.json`)) {
    await run(['api', '--method', 'DELETE', `repos/${repository}/releases/assets/${asset.id}`], env);
  }
  for (const name of names) {
    const uploaded = await upload({ repository, id, name, file: path.join(directory, name), env });
    verifyAssetDigests([receipt.files.find(file => file.name === name)], [uploaded], repository);
  }
  const completed = `qualification-${receipt.platform}.json`;
  await upload({ repository, id, name: completed, file: path.join(directory, 'dry-run-evidence.json'), env });
}

export async function sealDraft({ tag, sourceSha, runId, repository, env, run = gh }) {
  const draft = await readDraft(qualificationTag(tag, runId), repository, env, run) ?? await readDraft(tag, repository, env, run);
  const id = admitDraft(draft, tag, sourceSha, runId, repository);
  await run(['api', '--method', 'PATCH', `repos/${repository}/releases/${id}`, '-f',
    `tag_name=${draft.tagName}`, '-f', `body=${draftNotes(sourceSha, runId)}\n<!-- qualification-complete -->`], env);
  const sealed = await readDraft(draft.tagName, repository, env, run);
  admitDraft(sealed, tag, sourceSha, runId, repository);
  if (!sealed.body.includes('<!-- qualification-complete -->')) throw new Error('Qualification draft seal was not retained');
}

export async function qualificationRepositories(action) {
  for (const [repository, token] of [[sourceRepository(), process.env.GH_TOKEN], ['rollacode/codeterm-releases', process.env.MIRROR_TOKEN]]) {
    if (!token) throw new Error(`Missing token for ${repository}`);
    await action(repository, { ...process.env, GH_TOKEN: token });
  }
}

export function requireQualificationPush(tag, sourceSha, runId, env = process.env) {
  qualificationTag(tag, runId);
  draftNotes(sourceSha, runId);
  if (env.GITHUB_EVENT_NAME !== 'push' || env.GITHUB_REF !== RELEASE_REF ||
      env.GITHUB_SHA !== sourceSha || env.GITHUB_RUN_ID !== String(runId) ||
      env.GITHUB_ACTOR !== 'rollacode' || env.GITHUB_TRIGGERING_ACTOR !== 'rollacode') throw new Error('Not an owner qualification push');
}

export async function resetPlatform({ tag, sourceSha, runId, platform, repository, env, run = gh }) {
  const releases = (await run(['api', '--paginate', '--jq', '.[] | tojson',
    `repos/${repository}/releases?per_page=100`], env)).split('\n').filter(Boolean).map(JSON.parse);
  const prefix = `qualification-${tag}-`;
  const candidates = releases.filter(release => release.draft && release.tag_name.startsWith(prefix));
  if (candidates.some(release => BigInt(release.tag_name.slice(prefix.length)) > BigInt(runId))) throw new Error('Superseded qualification run');
  const draft = await readDraft(qualificationTag(tag, runId), repository, env, run);
  const id = admitDraft(draft, tag, sourceSha, runId, repository);
  await run(['api', '--method', 'PATCH', `repos/${repository}/releases/${id}`, '-f',
    `tag_name=${draft.tagName}`, '-f', `body=${draftNotes(sourceSha, runId)}`], env);
  admitDraft(await readDraft(draft.tagName, repository, env, run), tag, sourceSha, runId, repository);
  for (const old of candidates.filter(release => BigInt(release.tag_name.slice(prefix.length)) < BigInt(runId))) {
    try { await run(['api', '--method', 'DELETE', `repos/${repository}/releases/${old.id}`], env); }
    catch (error) { if (!/HTTP 404/.test(error.stderr ?? error.message)) throw error; }
  }
  const assets = (await run(['api', '--paginate', '--jq', '.[] | tojson',
    `repos/${repository}/releases/${id}/assets?per_page=100`], env)).split('\n').filter(Boolean).map(JSON.parse);
  for (const asset of assets.filter(asset => ownsReleaseAsset(asset.name, platform) ||
      asset.name === `qualification-${platform}.json` || asset.name.startsWith(`updater-${platform}`))) {
    await run(['api', '--method', 'DELETE', `repos/${repository}/releases/assets/${asset.id}`], env);
  }
}

async function main([verb, tag, sourceSha, runId, platform]) {
  requireQualificationPush(tag, sourceSha, runId);
  const current = (await gh(['api', `repos/${sourceRepository()}/git/ref/heads/${RELEASE_BRANCH}`, '--jq', '.object.sha'])).trim();
  if (current !== sourceSha) throw new Error('Superseded release head');
  if (verb === 'prepare') await qualificationRepositories((repository, env) => prepareDraft({ tag, sourceSha, runId, repository, env }));
  else if (verb === 'seal') await qualificationRepositories((repository, env) => sealDraft({ tag, sourceSha, runId, repository, env }));
  else if (verb === 'reset') await qualificationRepositories((repository, env) => resetPlatform({ tag, sourceSha, runId, platform, repository, env }));
  else throw new Error('Unknown draft operation');
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2)).catch(error => { console.error(error.message); process.exitCode = 1; });
}
