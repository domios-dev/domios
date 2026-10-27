import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import test from 'node:test';
import { checkLane, classifyScope, scanTree } from '../rename-guard.mjs';

const marker = ['code', 'term'].join('');
const script = path.resolve('scripts/rename-guard.mjs');
const allowlistPath = path.resolve('scripts/rename-guard-allowlist.json');

function fixture(t) {
  const root = mkdtempSync(path.join(os.tmpdir(), 'rename-guard-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  execFileSync('git', ['init', '-b', 'develop'], { cwd: root, stdio: 'ignore' });
  execFileSync('git', ['config', 'user.name', 'Rename Guard Fixture'], { cwd: root });
  execFileSync('git', ['config', 'user.email', 'rename-guard@example.invalid'], { cwd: root });
  return root;
}

function write(root, file, body) {
  const target = path.join(root, file);
  mkdirSync(path.dirname(target), { recursive: true });
  writeFileSync(target, body);
}

function commit(root, message) {
  execFileSync('git', ['add', '-A'], { cwd: root });
  execFileSync('git', ['commit', '-m', message], { cwd: root, stdio: 'ignore' });
}

function cli(root, allowlist, args) {
  return spawnSync(process.execPath, [script, '--root', root, '--allowlist', allowlist, ...args], {
    encoding: 'utf8', timeout: 10_000,
  });
}

function writeAllowlist(root, rules) {
  const file = path.join(root, 'allowlist.json');
  writeFileSync(file, JSON.stringify({ rules }));
  return file;
}

test('a seeded unallowlisted path or content hit fails; report mode records but does not fail', (t) => {
  const root = fixture(t);
  write(root, 'README.md', `This seeded ${marker} hit must fail.\n`);
  commit(root, 'seed unallowlisted identity');
  const rules = writeAllowlist(root, []);
  const strict = cli(root, rules, ['--scope', 'docs']);
  assert.equal(strict.status, 1, strict.stdout + strict.stderr);
  assert.match(strict.stdout, /FAIL docs README\.md/);
  const report = cli(root, rules, ['--scope', 'docs', '--report']);
  assert.equal(report.status, 0, report.stdout + report.stderr);
  assert.match(report.stdout, /docs: files=1 occurrences=1 violations=1/);
  assert.doesNotMatch(report.stdout, /FAIL /);
});

test('an exact path and literal allowlist rule accepts only the named occurrence', (t) => {
  const root = fixture(t);
  write(root, 'README.md', `Upstream rollacode/${marker} stays.\n`);
  commit(root, 'seed allowlisted upstream');
  const rules = writeAllowlist(root, [{ path: 'README.md', literal: `rollacode/${marker}`, reason: 'canonical source repository' }]);
  const accepted = cli(root, rules, ['--scope', 'docs']);
  assert.equal(accepted.status, 0, accepted.stdout + accepted.stderr);
  assert.match(accepted.stdout, /docs: files=1 occurrences=1 violations=0/);
  write(root, 'README.md', `Upstream rollacode/${marker}; second ${marker}.\n`);
  const extended = cli(root, rules, ['--scope', 'docs']);
  assert.equal(extended.status, 1, extended.stdout + extended.stderr);
  assert.match(extended.stdout, /occurrences=2 violations=1/);
});

test('scope rules map identity, CLI, crate, native and remaining files separately', () => {
  assert.equal(classifyScope(`src-tauri/crates/${marker}-paths/src/lib.rs`), 'identity');
  assert.equal(classifyScope('src-tauri/crates/domios-paths/src/lib.rs'), 'identity');
  assert.equal(classifyScope('cli/src/cmd/status.rs'), 'cli');
  assert.equal(classifyScope('src-tauri/crates/domios-types/Cargo.toml'), 'crates');
  assert.equal(classifyScope('src-tauri/tauri.windows.conf.json'), 'native');
  assert.equal(classifyScope('docs/architecture.md'), 'docs');
});

test('checked-in relaunch exceptions preserve rejection inputs without allowing old product prose', (t) => {
  const root = fixture(t);
  const file = `src-tauri/crates/${marker}-paths/src/relaunch_env.rs`;
  write(root, file, [
    'const MARKER: &str = "CODETERM";',
    'const PREFIX: &str = "CODETERM_";',
    'const FORMER_TERM: &str = "CodeTerm";',
    'const FORMER_ROOT: &str = ".codeterm";',
    'const FORMER_DEV_ROOT: &str = ".codeterm-dev";',
  ].join('\n'));
  commit(root, 'seed retained relaunch rejection inputs');
  const accepted = cli(root, allowlistPath, ['--scope', 'identity']);
  assert.equal(accepted.status, 0, accepted.stdout + accepted.stderr);
  write(root, file, `CodeTerm still owns the product data root.\n`);
  const prose = cli(root, allowlistPath, ['--scope', 'identity']);
  assert.equal(prose.status, 1, prose.stdout + prose.stderr);
  write(root, file, `const FORMER_ROOT: &str = ".codeterm-other";\n`);
  const sibling = cli(root, allowlistPath, ['--scope', 'identity']);
  assert.equal(sibling.status, 1, sibling.stdout + sibling.stderr);
});

test('each checked-in allowlist exception has a reason and Firebase values match the tracked recipe', () => {
  const parsed = JSON.parse(readFileSync(allowlistPath, 'utf8'));
  assert.ok(parsed.rules.length >= 9);
  for (const rule of parsed.rules) {
    assert.ok(rule.path, 'allowlist path is required');
    assert.ok(rule.literal, 'allowlist literal is required');
    assert.ok(rule.reason.trim(), `allowlist reason missing for ${rule.path}`);
  }
  const recipe = readFileSync(path.resolve('evals/qa/cases/CUR-012-installed-sign-in/assets/legacy-recipe.md'), 'utf8');
  for (const id of ['domios-d9357.web.app', 'us-central1-domios-d9357.cloudfunctions.net']) {
    assert.ok(recipe.includes(id), `verified Firebase value not present in the QA recipe: ${id}`);
    assert.ok(parsed.rules.some((rule) => rule.literal === id && rule.path.endsWith('legacy-recipe.md')));
  }
  const literals = parsed.rules.map((rule) => rule.literal.toLowerCase());
  for (const repository of ['rollacode/', 'releases', 'plugins']) {
    assert.ok(literals.some((literal) => literal.includes(repository)), `missing allowlist literal for ${repository}`);
  }
  for (const historyPath of ['captures/**', 'evals/**/evidence/**', 'docs/release/**', '.research/**']) {
    assert.ok(parsed.rules.some((rule) => rule.path === historyPath && rule.literal.toLowerCase() === marker), `missing historical path ${historyPath}`);
  }
  const buildScript = readFileSync(path.resolve('build.sh'), 'utf8');
  assert.match(buildScript, /sg_run node scripts\/rename-guard\.mjs --scope all --report/);
});

test('scan reports every scope and scans tracked paths as well as contents', (t) => {
  const root = fixture(t);
  write(root, `src-tauri/crates/${marker}-paths/src/lib.rs`, 'clean source');
  write(root, 'docs/README.md', `content contains ${marker}`);
  commit(root, 'seed scoped paths');
  const rules = writeAllowlist(root, []);
  const result = scanTree({ root, allowlist: rules, scope: 'all' });
  assert.equal(result.counts.crates.files, 1);
  assert.equal(result.counts.crates.occurrences, 1);
  assert.equal(result.counts.identity.files, 0);
  assert.equal(result.counts.docs.violations, 1);
  assert.equal(result.counts.all.files, 2);
  const report = cli(root, rules, ['--scope', 'all', '--report']);
  assert.equal(report.status, 0, report.stdout + report.stderr);
  for (const scope of ['identity', 'cli', 'crates', 'native', 'docs', 'all']) assert.match(report.stdout, new RegExp(`^${scope}:`, 'm'));
});

test('identity files keep runtime identity in identity and route phase-2 names by occurrence', (t) => {
  const root = fixture(t);
  write(root, `src-tauri/crates/${marker}-paths/src/lib.rs`, [
    'use codeterm_paths::data_dir;',
    'const INSTANCE: &str = "CODETERM_INSTANCE";',
    'const ABOUT: &str = "CodeTerm data";',
    'const MAGIC: &[u8] = b"CODETERM1";',
    '// codeterm#123 tracks the historical issue.',
  ].join('\n'));
  commit(root, 'seed mixed identity and phase-2 names');
  const rules = writeAllowlist(root, []);
  const result = scanTree({ root, allowlist: rules, scope: 'all' });
  assert.deepEqual(result.counts.identity, { files: 1, occurrences: 1, violations: 1 });
  assert.deepEqual(result.counts.cli, { files: 1, occurrences: 1, violations: 1 });
  assert.deepEqual(result.counts.crates, { files: 1, occurrences: 3, violations: 3 });
  assert.deepEqual(result.counts.docs, { files: 1, occurrences: 1, violations: 1 });
  assert.equal(result.counts.all.occurrences, 6);
});

test('lane checker rejects out-of-list edits, reports occurrences, overlaps and unowned files', (t) => {
  const root = fixture(t);
  write(root, 'src/owned.txt', `baseline ${marker}\n`);
  write(root, 'docs/other.txt', 'baseline\n');
  commit(root, 'lane base');
  execFileSync('git', ['checkout', '-b', 'worker'], { cwd: root, stdio: 'ignore' });
  write(root, 'src/owned.txt', `changed ${marker}\n`);
  write(root, 'docs/other.txt', `changed ${marker}\n`);
  commit(root, 'edit owned and foreign files');
  const manifest = path.join(root, 'ownership.json');
  writeFileSync(manifest, JSON.stringify({ phases: { '3': [
    { phase: '3', key: 'COD-1', id: 'owner', handle: 'worker', paths: ['src', 'src/owned.txt'] },
    { phase: '3', key: 'COD-2', id: 'other', handle: 'other', paths: ['src/owned.txt'] },
  ] } }));
  const result = checkLane({ root, ownership: manifest, taskKey: 'COD-1' });
  assert.deepEqual(result.pathFailures, ['docs/other.txt']);
  assert.ok(result.occurrences.some((entry) => entry.file === 'src/owned.txt'));
  assert.deepEqual(result.diagnostics['3'].overlaps, [{ file: 'src/owned.txt', tasks: ['COD-1', 'COD-2'] }]);
  assert.deepEqual(result.diagnostics['3'].unowned, ['docs/other.txt']);
});

test('only Herald bridge additions and Closer bridge removals are admitted', (t) => {
  for (const [handle, baseline, body, failures] of [
    ['backend-1-15-0-4', 'base\n', 'base\nrename-bridge\n', 0],
    ['infra-1-15-0-4', 'base\nrename-bridge\n', 'base\n', 0],
    ['docs-1-15-0', 'base\n', 'base\nrename-bridge\n', 1],
  ]) {
    const root = fixture(t);
    write(root, 'build.sh', baseline);
    commit(root, 'lane base');
    execFileSync('git', ['checkout', '-b', 'worker'], { cwd: root, stdio: 'ignore' });
    write(root, 'build.sh', body);
    commit(root, 'bridge change');
    const manifest = path.join(root, 'ownership.json');
    writeFileSync(manifest, JSON.stringify({ phases: { '2': [{ phase: '2', key: 'COD-X', id: 'owner', handle, paths: ['build.sh'] }] } }));
    const result = checkLane({ root, ownership: manifest, taskKey: 'COD-X' });
    assert.equal(result.bridgeFailures.length, failures);
  }
});
