import { readFileSync } from 'node:fs';
import path from 'node:path';
import { pathToFileURL } from 'node:url';

export const RELEASE_BRANCH = 'release/next';
export const RELEASE_REF = 'refs/heads/' + RELEASE_BRANCH;

export function releaseTag({ version }, requestedTag = '') {
  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error('Expected an X.Y.Z version in package.json');
  }
  const tag = 'v' + version;
  if (requestedTag && requestedTag !== tag) throw new Error(`Expected tag ${tag} for package.json, got ${requestedTag}`);
  return tag;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log(releaseTag(JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8')), process.argv[2]));
}
