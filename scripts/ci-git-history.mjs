// Caller: CI/release workflows; scripts/tests/ci-checkout.test.mjs and ci-git-history.test.mjs
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';

export const HISTORY_PURPOSES = {
  policy: { reason: 'Guard merge bases and release provider-boundary ancestry', refs: ['refs/heads/develop:refs/remotes/origin/develop'] },
  baseline: { reason: 'Compare warning and slow-test baselines at the develop fork point', refs: ['refs/heads/develop:refs/remotes/origin/develop'] },
  release: { reason: 'Find the previous reachable product version', refs: ['refs/tags/v*:refs/tags/v*'] },
  publication: { reason: 'Resolve issue closure boundaries and the existing product tag', refs: ['refs/tags/release-*:refs/tags/release-*', 'refs/tags/v*:refs/tags/v*'] },
  promote: { reason: 'Verify and merge release ancestry into master and develop', refs: ['refs/heads/master:refs/remotes/origin/master', 'refs/heads/develop:refs/remotes/origin/develop'] },
  master: { reason: 'Prove publication source is merged into master', refs: ['refs/heads/master:refs/remotes/origin/master'] },
  ancestry: { reason: 'Verify reviewed commit ancestry within the exact source history', refs: [] },
};

export function historyArguments(purpose, sha, shallow, env = {}) {
  const policy = HISTORY_PURPOSES[purpose];
  if (!policy) throw new Error('Unknown scoped history purpose: ' + purpose);
  if (!/^[a-f0-9]{40}$/.test(sha)) throw new Error('Scoped history requires an exact source SHA');
  const refs = [sha, ...policy.refs];
  if (purpose === 'policy') {
    for (const name of ['PR_BASE_SHA', 'PR_HEAD_SHA', 'PUSH_BEFORE']) {
      const value = env[name];
      if (/^[a-f0-9]{40}$/.test(value || '') && !/^0+$/.test(value)) refs.push(value);
    }
  }
  return ['fetch', '--no-tags', '--filter=blob:none', ...(shallow ? ['--unshallow'] : []), 'origin', ...new Set(refs)];
}

export function githubGitEnvironment(env) {
  const result = { ...env };
  if (!env.GH_TOKEN) return result;
  const index = Number(env.GIT_CONFIG_COUNT || 0);
  const key = 'http.https://github.com/.extraheader';
  result.GIT_CONFIG_COUNT = String(index + 2);
  result['GIT_CONFIG_KEY_' + index] = key;
  result['GIT_CONFIG_VALUE_' + index] = '';
  result['GIT_CONFIG_KEY_' + (index + 1)] = key;
  result['GIT_CONFIG_VALUE_' + (index + 1)] = 'AUTHORIZATION: basic ' + Buffer.from('x-access-token:' + env.GH_TOKEN).toString('base64');
  return result;
}

export function fetchHistory(purpose, env = process.env, git = execFileSync) {
  const source = git('git', ['rev-parse', 'HEAD'], { encoding: 'utf8' }).trim();
  const shallow = git('git', ['rev-parse', '--is-shallow-repository'], { encoding: 'utf8' }).trim() === 'true';
  const args = historyArguments(purpose, source, shallow, env);
  const fetchEnv = githubGitEnvironment(env);
  console.log('Scoped history: ' + HISTORY_PURPOSES[purpose].reason);
  git('git', args, { env: fetchEnv, stdio: 'inherit' });
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) fetchHistory(process.argv[2]);
