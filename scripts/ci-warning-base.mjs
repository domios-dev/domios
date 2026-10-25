// Caller: rust-warnings-summary.yml; scripts/tests/ci-checkout.test.mjs
import fs from 'node:fs';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { githubGitEnvironment } from './ci-git-history.mjs';

export async function fetchWarningBase({ env = process.env, request = fetch, git = execFileSync } = {}) {
  const repository = env.GITHUB_REPOSITORY;
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository || '')) throw new Error('GITHUB_REPOSITORY is required');
  const sha = git('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error('Warning baseline requires an exact source SHA');
  const response = await request('https://api.github.com/repos/' + repository + '/compare/develop...' + sha, {
    headers: { Authorization: 'Bearer ' + env.GH_TOKEN, Accept: 'application/vnd.github+json' },
    signal: AbortSignal.timeout(30_000),
  });
  if (!response.ok) throw new Error('Cannot resolve warning fork point: HTTP ' + response.status);
  const base = (await response.json()).merge_base_commit?.sha;
  if (!/^[a-f0-9]{40}$/.test(base || '')) throw new Error('Missing warning merge-base SHA');
  const fetchEnv = githubGitEnvironment(env);
  git('git', ['fetch', '--no-tags', '--depth=1', 'origin', base], { env: fetchEnv, stdio: 'inherit' });
  if (env.GITHUB_ENV) fs.appendFileSync(env.GITHUB_ENV, 'WARNING_BASE_SHA=' + base + '\n');
  return base;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) await fetchWarningBase();
