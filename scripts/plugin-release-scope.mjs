import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import { isDeepStrictEqual } from 'node:util';
import { pathToFileURL } from 'node:url';
import { githubGitEnvironment } from './ci-git-history.mjs';
import { compareVersions } from './plugin-release-metadata.mjs';

const sharedFixtures = new Map([['opencode', ['tests/fixtures/runtime-http-sse.json']]]);
const shaPattern = /^[a-f0-9]{40}$/;
const segmentPattern = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;
const versionPattern = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;
const channelFile = 'plugins/channel.json';

export function classifyPluginDiff(files, before, after) {
  if (!files.includes(channelFile) || before.plugins.length !== after.plugins.length) return null;
  const changed = before.plugins.flatMap((entry, index) =>
    isDeepStrictEqual(entry, after.plugins[index]) ? [] : [index]);
  if (changed.length !== 1) return null;
  const index = changed[0];
  const { id, path, version } = before.plugins[index];
  const next = after.plugins[index];
  if (!segmentPattern.test(id) || !segmentPattern.test(path) || !versionPattern.test(next.version)
      || compareVersions(next.version, version) <= 0) return null;
  const allowed = structuredClone(before);
  allowed.plugins[index].version = next.version;
  allowed.plugins[index].minCodeterm = next.minCodeterm;
  if (!isDeepStrictEqual(allowed, after)) return null;
  const prefix = 'plugins/' + path + '/';
  const fixtures = sharedFixtures.get(id) ?? [];
  if (!files.some(file => file.startsWith(prefix)) ||
      files.some(file => file !== channelFile && !file.startsWith(prefix) && !fixtures.includes(file))) return null;
  return { id, tag: 'plugin-' + id + '-v' + next.version };
}

function git(...args) {
  return execFileSync('git', args, { encoding: 'utf8', env: githubGitEnvironment(process.env) });
}

export function fetchPluginReleaseBase(ref) {
  if (ref !== 'refs/heads/master' && !shaPattern.test(ref)) throw new Error('Invalid plugin release base');
  git('fetch', '--no-tags', '--depth=1', 'origin', ref);
  return git('rev-parse', 'FETCH_HEAD').trim();
}

export function pluginReleaseBetween(base, head) {
  if (!shaPattern.test(base) || !shaPattern.test(head)) throw new Error('Plugin scope requires exact SHAs');
  const files = git('diff', '--name-only', '--no-renames', '-z', base, head).split('\0').filter(Boolean);
  if (!files.includes(channelFile) || files.some(file => !file.startsWith('plugins/') &&
      ![...sharedFixtures.values()].some(fixtures => fixtures.includes(file)))) return null;
  const channel = sha => JSON.parse(git('show', sha + ':' + channelFile));
  return classifyPluginDiff(files, channel(base), channel(head));
}

export function directPluginId(headRef) {
  const prefix = 'release/plugin-';
  if (!headRef.startsWith(prefix)) return null;
  const id = headRef.slice(prefix.length);
  return segmentPattern.test(id) ? id : null;
}

export function checkDirectPluginRelease(base, head, headRef) {
  const plugin = pluginReleaseBetween(base, head);
  if (!plugin || plugin.id !== directPluginId(headRef))
    throw new Error('Plugin PR must bump exactly its named plugin and touch only its owned files');
  git('diff', '--check', base, head);
  return plugin;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const base = fetchPluginReleaseBase('refs/heads/master');
  if (base !== process.env.PR_BASE_SHA) throw new Error('Update the plugin PR to the current master head');
  const plugin = checkDirectPluginRelease(base, process.env.PR_HEAD_SHA, process.env.PR_HEAD_REF);
  fs.appendFileSync(process.env.GITHUB_OUTPUT, 'id=' + plugin.id + '\ntag=' + plugin.tag + '\n');
}
