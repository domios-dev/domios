import { execFileSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

export const HISTORICAL_SOURCE_REPOSITORIES = JSON.parse(fs.readFileSync(
  new URL('./source-repository-aliases.json', import.meta.url), 'utf8'));

export function repositoryFromRemote(remote) {
  const match = remote.trim().match(/^(?:git@github\.com:|https:\/\/github\.com\/)([^/]+\/[^/]+?)(?:\.git)?$/);
  if (!match) throw new Error('Set DOMIOS_SOURCE_REPOSITORY to the source owner/repository');
  return match[1];
}

export function sourceRepository(env = process.env, readRemote = () => execFileSync(
  'git', ['config', '--get', 'remote.origin.url'], { cwd: path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..'), encoding: 'utf8' })) {
  const repository = env.GITHUB_REPOSITORY || env.DOMIOS_SOURCE_REPOSITORY || repositoryFromRemote(readRemote());
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid source repository: ' + repository);
  return repository;
}

if (process.argv[1] && import.meta.url === pathToFileURL(path.resolve(process.argv[1])).href) {
  console.log(sourceRepository());
}
