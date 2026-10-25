import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import test from 'node:test';
import { fetchHistory } from '../ci-git-history.mjs';

test('a real shallow checkout gains baseline ancestry without unrelated branches or tags', () => {
  const directory = fs.mkdtempSync(path.join(os.tmpdir(), 'ci-history-'));
  const source = path.join(directory, 'source'), checkout = path.join(directory, 'checkout');
  const env = { ...process.env, GIT_AUTHOR_NAME:'Fixture', GIT_AUTHOR_EMAIL:'fixture@example.invalid',
    GIT_COMMITTER_NAME:'Fixture', GIT_COMMITTER_EMAIL:'fixture@example.invalid' };
  const git = (cwd, args, options={}) => execFileSync('git',args,{...options,cwd,env,encoding:'utf8',stdio:['ignore','pipe','pipe']});
  try {
    git(directory,['init','--initial-branch=develop',source]);
    fs.writeFileSync(path.join(source,'fixture.txt'),'base');
    git(source,['add','.']);git(source,['commit','-m','fixture: base']);
    const base = git(source,['rev-parse','HEAD']).trim();
    git(source,['checkout','-b','release']);
    fs.writeFileSync(path.join(source,'fixture.txt'),'release');
    git(source,['commit','-am','fixture: source']);
    git(source,['branch','unrelated']);git(source,['tag','unrelated-tag']);
    git(directory,['clone','--no-tags','--depth=1','--branch=release',pathToFileURL(source).href,checkout]);
    assert.equal(git(checkout,['rev-parse','--is-shallow-repository']).trim(),'true');
    fetchHistory('baseline',{},(_program,args,options)=>git(checkout,args,options));
    assert.equal(git(checkout,['merge-base','HEAD','origin/develop']).trim(),base);
    assert.equal(git(checkout,['rev-parse','--is-shallow-repository']).trim(),'false');
    assert.equal(git(checkout,['for-each-ref','--format=%(refname)','refs/remotes/origin/unrelated','refs/tags']).trim(),'');
  } finally { fs.rmSync(directory,{recursive:true,force:true}); }
});
