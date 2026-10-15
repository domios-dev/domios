import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { chmodSync, existsSync, mkdirSync, mkdtempSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';
import { allowlistPath, checkBoundary, detectOccurrences, isCoreSource, readBaseline, scanWorkingTree, seedAllowlist, validateAllowlist } from '../check-provider-boundary.mjs';

const checker = fileURLToPath(new URL('../check-provider-boundary.mjs', import.meta.url));
const file = 'src-tauri/src/runtime.rs';

function source(root, path, body) {
  mkdirSync(dirname(join(root, path)), { recursive: true });
  writeFileSync(join(root, path), body);
}

function repository(run) {
  const root = mkdtempSync(join(tmpdir(), 'provider-boundary-'));
  const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8' }).trim();
  const commit = () => {
    git('add', '-A');
    git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--quiet', '-m', 'fixture');
    return git('rev-parse', 'HEAD');
  };
  try {
    git('init', '--quiet');
    source(root, file, 'fn codex_runtime() {}\n');
    const seedBase = commit();
    const document = seedAllowlist(scanWorkingTree(root));
    source(root, allowlistPath, JSON.stringify(document));
    run({ root, git, commit, seedBase, document,
      check: base => spawnSync(process.execPath, [checker, '--base', base], { cwd: root, encoding: 'utf8' }),
    });
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

test('detects literal ids, casing, identifier and environment variants', () => {
  const tokens = ['codex', 'CODEX', 'CodexRpc', 'codex_rpc', 'CLAUDE_CONFIG_DIR', 'ClaudeCode', 'claude', 'OpenCode', 'OPEN_CODE', 'open-code', 'opencode_db', 'AiderProvider', 'AIDER_HOME', 'aider', 'GooseSession', 'GOOSE_HOME', 'goose'];
  const source = tokens.map(token => `let value = "${token}";`).join('\n');
  assert.deepEqual(detectOccurrences(file, source).map(hit => hit.token), tokens);
  assert.equal(detectOccurrences(file, 'let agent_runtime = "chat_backend";').length, 0);
});

test('source-backed production and test tokens are included', () => repository(({ root }) => {
  source(root, 'src-tauri/crates/example/tests/provider.rs', 'let token = "CLAUDE_CONFIG_DIR";');
  source(root, 'crates/example/src/fixtures.json', '{"provider":"OpenCode"}');
  source(root, 'plugins/provider-codex/src/plugin.ts', 'const codex = "codex";');
  const occurrences = scanWorkingTree(root);
  assert.deepEqual(occurrences.map(hit => hit.token).sort(), ['CLAUDE_CONFIG_DIR', 'OpenCode', 'codex_runtime']);
  for (const path of ['src-tauri/src/runtime/tests.rs', 'src-tauri/crates/example/tests/provider.rs', 'crates/example/src/fixtures.json']) assert.ok(isCoreSource(path));
  for (const path of ['plugins/provider-codex/src/plugin.ts', 'src-tauri/pty-fork/src/lib.rs', 'prod-docs/CONFIG.md']) assert.equal(isCoreSource(path), false);
}));

test('moving and reformatting existing tokens within a file preserves its allowance', () => {
  const original = detectOccurrences(file, 'before\nCodexRuntime\nafter\n');
  const document = seedAllowlist(original);
  const changed = detectOccurrences(file, 'unrelated new line\nrenamed_before();\n  CodexRuntime  \nrenamed_after();\n');
  assert.deepEqual(checkBoundary(changed, document, document.entries), []);
});

test('a file/token count cannot allow repeated copies or another file', () => {
  const original = detectOccurrences(file, 'x\ncodex\nx\n');
  const document = seedAllowlist(original);
  assert.deepEqual(checkBoundary(original, document, document.entries), []);
  const duplicate = detectOccurrences(file, 'x\ncodex\nx\ncodex\nx\n');
  assert.ok(checkBoundary(duplicate, document, document.entries).some(error => error.includes('added occurrence')));
  assert.ok(checkBoundary(detectOccurrences('crates/runtime/src/lib.rs', 'x\ncodex\nx\n'), document, document.entries).length > 0);
});

test('CLI transfers an exact allowance across a git rename, including a facade left behind', () => repository(({ root, git, commit, document, check }) => {
  const base = commit();
  const moved = 'src-tauri/crates/codeterm-leaf/src/runtime.rs';
  mkdirSync(dirname(join(root, moved)), { recursive: true });
  renameSync(join(root, file), join(root, moved));
  source(root, file, 'pub use codeterm_leaf::runtime::*;\n');
  source(root, allowlistPath, JSON.stringify({ ...document, entries: document.entries.map(entry => ({ ...entry, file: moved })) }));
  git('add', '-A');
  assert.equal(check(base).status, 0, check(base).stderr);
  source(root, moved, 'fn codex_runtime() {}\nfn codex_runtime() {}\n');
  assert.match(check(base).stderr, /added occurrence|increased baseline/);
}));

test('CLI does not transfer an allowance to a copy', () => repository(({ root, commit, document, check }) => {
  const base = commit();
  const copied = 'src-tauri/crates/codeterm-leaf/src/runtime.rs';
  source(root, copied, 'fn codex_runtime() {}\n');
  source(root, allowlistPath, JSON.stringify({ ...document, entries: [...document.entries, { ...document.entries[0], file: copied }] }));
  assert.match(check(base).stderr, /increased baseline/);
}));

test('CLI divides each renamed token budget between its facade and destination without growth', () => repository(({ root, git, commit, check }) => {
  const original = 'const CODEX_HOME: &str = "";\nfn codex_rpc() {}\nfn codex_rpc() {}\n';
  source(root, file, original);
  const document = seedAllowlist(scanWorkingTree(root));
  source(root, allowlistPath, JSON.stringify(document));
  const base = commit();
  const moved = 'src-tauri/crates/codeterm-leaf/src/runtime.rs';
  mkdirSync(dirname(join(root, moved)), { recursive: true });
  renameSync(join(root, file), join(root, moved));
  source(root, file, 'fn codex_rpc() {}\n');
  source(root, moved, 'const CODEX_HOME: &str = "";\nfn codex_rpc() {}\n');
  const refresh = () => source(root, allowlistPath, JSON.stringify(seedAllowlist(scanWorkingTree(root))));
  refresh();
  git('add', '-A');
  const result = check(base);
  assert.equal(result.status, 0, result.stderr);
  source(root, moved, original);
  refresh();
  assert.match(check(base).stderr, /increased baseline/);
}));

test('lowered source count requires a lowered entry and cannot rise later', () => {
  const original = detectOccurrences(file, 'codex\ncodex\n');
  const document = seedAllowlist(original);
  assert.equal(document.entries.length, 1);
  assert.equal(document.entries[0].count, 2);
  const remaining = detectOccurrences(file, 'codex\n');
  assert.ok(checkBoundary(remaining, document, document.entries).some(error => error.includes('stale entry')));
  const lowered = seedAllowlist(remaining);
  assert.deepEqual(checkBoundary(remaining, lowered, document.entries), []);
  assert.ok(checkBoundary(original, document, lowered.entries).some(error => error.includes('increased baseline')));
});

test('forged and overbroad allowlist entries fail closed', () => {
  const original = seedAllowlist(detectOccurrences(file, 'codex'));
  for (const patch of [
    { file: 'src-tauri/src/**' }, { file: 'src-tauri/src/*.rs' }, { file: 'src-tauri/src/../runtime.rs' },
    { file: '/src-tauri/src/runtime.rs' }, { file: 'src-tauri/src\\runtime.rs' },
    { token: '*' }, { count: 0 }, { count: -1 }, { count: 1.5 }, { fingerprint: '*' }, { justification: '' }, { removalSlice: '' }, { wholeFile: true },
  ]) assert.throws(() => validateAllowlist({ version: 1, entries: [{ ...original.entries[0], ...patch }] }));
  assert.throws(() => validateAllowlist({ ...original, maximum: 10000 }));
  assert.throws(() => validateAllowlist({ version: 1, entries: [...original.entries, ...original.entries] }));
  const forged = { ...original.entries[0], token: 'ClaudeRuntime' };
  const errors = checkBoundary(detectOccurrences(file, 'codex'), { version: 1, entries: [forged] }, original.entries);
  assert.ok(errors.some(error => error.includes('unused entry')));
  assert.ok(errors.some(error => error.includes('increased baseline')));
});

test('CLI accepts a seed from target source and rejects new occurrences even when allowlisted', () => repository(({ root, seedBase, document, check }) => {
  assert.equal(check(seedBase).status, 0);
  source(root, 'crates/fixture/tests/test.rs', 'fn ClaudeTest() {}\n');
  assert.match(check(seedBase).stderr, /added occurrence/);
  source(root, allowlistPath, JSON.stringify(seedAllowlist(scanWorkingTree(root))));
  const result = check(seedBase);
  assert.equal(result.status, 1);
  assert.match(result.stderr, /increased baseline/);
  assert.equal(document.entries.length, 1);
}));

test('CLI rejects unused entries; removal with its entry passes and cannot later be restored', () => repository(({ root, commit, document, check }) => {
  const base = commit();
  source(root, file, 'fn agent_runtime() {}\n');
  assert.match(check(base).stderr, /unused entry/);
  source(root, allowlistPath, JSON.stringify({ version: 1, entries: [] }));
  assert.equal(check(base).status, 0);
  const shrunk = commit();
  source(root, file, 'fn codex_runtime() {}\n');
  source(root, allowlistPath, JSON.stringify(document));
  assert.match(check(shrunk).stderr, /increased baseline/);
}));

test('CLI refuses raised counts, overbroad entries and missing baseline refs', () => repository(({ root, commit, document, check }) => {
  const base = commit();
  document.entries[0].count++;
  source(root, allowlistPath, JSON.stringify(document));
  assert.match(check(base).stderr, /increased baseline/);
  document.entries[0].file = 'src-tauri/src/*.rs';
  source(root, allowlistPath, JSON.stringify(document));
  assert.match(check(base).stderr, /non-exact core source path/);
  assert.equal(check('missing-ref').status, 1);
}));

test('a branch cut before its base moved a file compares at the fork point', () => repository(({ root, git, commit, check }) => {
  const fork = commit();
  git('checkout', '--quiet', '-b', 'moved-base');
  const moved = 'src-tauri/crates/service-core/src/runtime.rs';
  mkdirSync(dirname(join(root, moved)), { recursive: true });
  git('mv', file, moved);
  source(root, allowlistPath, JSON.stringify(seedAllowlist(scanWorkingTree(root))));
  commit();
  git('checkout', '--quiet', '-b', 'release-cut', fork);
  source(root, 'README.md', 'release work\n');
  commit();
  const againstHead = checkBoundary(scanWorkingTree(root), JSON.parse(git('show', `HEAD:${allowlistPath}`)), readBaseline(root, 'moved-base'));
  assert.ok(againstHead.some(error => error.includes('increased baseline')));
  const result = check('moved-base');
  assert.equal(result.status, 0, result.stderr);
  assert.match(result.stdout, new RegExp(`fork point ${fork}`));
  source(root, 'crates/fixture/src/lib.rs', 'fn codex_leak() {}\n');
  assert.match(check('moved-base').stderr, /added occurrence/);
}));

test('CLI refuses a base with no fork point', () => repository(({ root, git, commit, check }) => {
  const own = commit();
  git('checkout', '--quiet', '--orphan', 'unrelated');
  source(root, 'README.md', 'unrelated\n');
  commit();
  git('checkout', '--quiet', '--force', own);
  const result = check('unrelated');
  assert.equal(result.status, 1);
  assert.match(result.stderr, /no fork point/);
}));

test('QA source gate propagates either guard failure without masking source-size failure', { skip: process.platform === 'win32' }, () => {
  const root = mkdtempSync(join(tmpdir(), 'provider-gate-wiring-'));
  const registry = fileURLToPath(new URL('../qa-gates.lib.sh', import.meta.url));
  const called = join(root, 'boundary-called');
  try {
    source(root, 'bash', '#!/bin/sh\nexit "${SIZE_EXIT}"\n');
    source(root, 'node', '#!/bin/sh\n: > "${BOUNDARY_CALLED}"\nexit "${BOUNDARY_EXIT}"\n');
    for (const binary of ['bash', 'node']) chmodSync(join(root, binary), 0o755);
    const run = (size, boundary) => spawnSync('/bin/bash', ['-c', 'source "$1"; qa_run_gate_body file-sizes', 'fixture', registry], {
      encoding: 'utf8',
      env: { ...process.env, PATH: root, QA_ROOT: root, SIZE_EXIT: String(size), BOUNDARY_EXIT: String(boundary), BOUNDARY_CALLED: called },
    });
    assert.equal(run(17, 0).status, 17);
    assert.equal(existsSync(called), false);
    assert.equal(run(0, 19).status, 19);
    assert.ok(existsSync(called));
    assert.equal(run(0, 0).status, 0);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
