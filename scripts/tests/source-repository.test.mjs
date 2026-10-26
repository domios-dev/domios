import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { HISTORICAL_SOURCE_REPOSITORIES, repositoryFromRemote, sourceRepository } from '../source-repository.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');

test('source repository follows runtime identity, local override or the checkout remote', () => {
  assert.equal(sourceRepository({ GITHUB_REPOSITORY: 'fixture/runtime', DOMIOS_SOURCE_REPOSITORY: 'fixture/local' }), 'fixture/runtime');
  assert.equal(sourceRepository({ DOMIOS_SOURCE_REPOSITORY: 'fixture/local' }), 'fixture/local');
  for (const remote of ['git@github.com:fixture/checkout.git', 'https://github.com/fixture/checkout.git']) {
    assert.equal(repositoryFromRemote(remote), 'fixture/checkout');
    assert.equal(sourceRepository({}, () => remote), 'fixture/checkout');
  }
  assert.throws(() => sourceRepository({ GITHUB_REPOSITORY: 'invalid' }), /Invalid source repository/);
});


test('historical issue aliases normalize to the runtime repository while foreign metadata is refused', t => {
  const directory = mkdtempSync(path.join(tmpdir(), 'source-repository-'));
  t.after(() => rmSync(directory, { recursive: true, force: true }));
  const file = path.join(directory, 'prs.json');
  const commits = path.join(directory, 'commits.json');
  writeFileSync(commits, '[]');
  for (const repository of [...HISTORICAL_SOURCE_REPOSITORIES, 'fixture/runtime', 'foreign/repository']) {
    writeFileSync(file, JSON.stringify([{ number: 1, url: 'https://github.com/fixture/runtime/pull/1', body: 'Repository-Issue: ' + repository + '#41' }]));
    const result = spawnSync(process.platform === 'win32' ? 'python' : 'python3',
      ['scripts/release_issue_contract.py', 'resolve', '--commits-file', commits, '--prs-file', file], {
        cwd: root, env: { ...process.env, GITHUB_REPOSITORY: 'fixture/runtime' }, encoding: 'utf8',
      });
    if (repository === 'foreign/repository') assert.notEqual(result.status, 0);
    else {
      assert.equal(result.status, 0, result.stderr);
      assert.equal(JSON.parse(result.stdout).repository_issues[0].reference, 'fixture/runtime#41');
    }
  }
});
