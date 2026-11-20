import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { githubGitEnvironment } from './ci-git-history.mjs';

const shaPattern = /^[a-f0-9]{40}$/;
const imagePath = /^docs\/readme\/[A-Za-z0-9_-][A-Za-z0-9_.-]*\.(?:png|jpe?g|gif|webp)$/i;

function allowedPath(path) {
  return path === 'README.md' || path.startsWith('docs/readme/') && imagePath.test(path);
}

function git(...args) {
  return execFileSync('git', args, { encoding: 'utf8', env: githubGitEnvironment(process.env) });
}

function checkImage(head, path) {
  const data = execFileSync('git', ['show', head + ':' + path], { maxBuffer: 32 * 1024 * 1024 });
  const extension = path.split('.').at(-1).toLowerCase();
  const valid = {
    png: data.subarray(0, 8).equals(Buffer.from('89504e470d0a1a0a', 'hex')),
    jpg: data.subarray(0, 3).equals(Buffer.from('ffd8ff', 'hex')),
    gif: /^GIF8[79]a$/.test(data.toString('ascii', 0, 6)),
    webp: data.toString('ascii', 0, 4) === 'RIFF' && data.toString('ascii', 8, 12) === 'WEBP',
  };
  if (!valid[extension === 'jpeg' ? 'jpg' : extension]) throw new Error('Invalid image contents: ' + path);
}

export function docsAssetLinks(readme) {
  const links = [];
  for (const match of readme.matchAll(/<(?:img|source)\b[^>]*>/gi)) {
    for (const [, name, , quoted, unquoted] of match[0].matchAll(/\s(src|srcset)\s*=\s*(?:(["'])(.*?)\2|([^"'\s>]+))/gi)) {
      const value = quoted ?? unquoted;
      if (name.toLowerCase() === 'srcset') links.push(...value.split(',').map(item => item.trim().split(/\s+/)[0]));
      else links.push(value.trim());
    }
  }
  for (const [, image, target] of readme.matchAll(/(!?)\[[^\]\n]*\]\(\s*<?([^>\s)]+)>?/g)) {
    if (image || /^(?:\.\/)?docs\/readme\//.test(target)) links.push(target);
  }
  for (const [, target] of readme.matchAll(/^\s{0,3}\[[^\]\n]+\]:\s*<?((?:\.\/)?docs\/readme\/[^>\s]+)>?/gm)) {
    links.push(target);
  }
  return [...new Set(links.filter(link => !/^(?:[a-z][a-z\d+.-]*:|\/\/)/i.test(link))
    .map(link => decodeURIComponent(link.split(/[?#]/)[0]).replace(/^\.\//, '')))];
}

export function checkDocsRelease(base, head) {
  if (!shaPattern.test(base) || !shaPattern.test(head)) throw new Error('Docs scope requires exact SHAs');
  const changed = git('diff', '--name-only', '--no-renames', '-z', base, head).split('\0').filter(Boolean);
  if (!changed.length || changed.some(path => !allowedPath(path)))
    throw new Error('Docs PR may change only README.md and image files in docs/readme');
  git('diff', '--check', base, head);
  const files = new Map(git('ls-tree', '-rz', '--full-tree', head, '--', 'README.md', 'docs/readme/')
    .split('\0').filter(Boolean).map(entry => {
      const tab = entry.indexOf('\t');
      return [entry.slice(tab + 1), entry.slice(0, tab).split(' ')[0]];
    }));
  if (files.get('README.md') !== '100644') throw new Error('README.md must be a regular file');
  for (const path of changed) {
    if (!files.has(path)) continue;
    if (files.get(path) !== '100644') throw new Error('Docs file must be regular: ' + path);
    if (path !== 'README.md') checkImage(head, path);
  }
  for (const link of docsAssetLinks(git('show', head + ':README.md'))) {
    if (link === 'README.md' || !allowedPath(link) || files.get(link) !== '100644')
      throw new Error('README local image does not resolve to a regular docs/readme asset: ' + link);
  }
  return changed;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  git('fetch', '--no-tags', '--depth=1', 'origin', 'refs/heads/master');
  const base = git('rev-parse', 'FETCH_HEAD').trim();
  if (base !== process.env.PR_BASE_SHA) throw new Error('Update the docs PR to current master');
  console.log('Verified docs scope, whitespace and local assets for ' + checkDocsRelease(base, process.env.PR_HEAD_SHA).join(', '));
}
