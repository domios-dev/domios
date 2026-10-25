import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { parse } from 'yaml';
import { warningArtifactCommand } from '../ci-rust-warning-artifact.mjs';

const env = { WARNING_INPUT: 'C:/job path/compiler log.jsonl', WARNING_PLATFORM: 'Windows',
  WARNING_TARGET: 'x86_64-pc-windows-msvc', WARNING_COMPLETE: 'false',
  WARNING_FILE: 'rust-warnings-Windows-x86_64-pc-windows-msvc.json', GITHUB_RUN_ID: '12345' };

test('warning collector forwards spaced paths as single arguments without a custom shell', () => {
  const command = warningArtifactCommand('collect', env, 'win32');
  assert.equal(command.program, process.execPath);
  assert.equal(command.args[command.args.indexOf('--input') + 1], env.WARNING_INPUT);
  assert.equal(command.args.at(-1), `ci-rust-warnings/${env.WARNING_FILE}`);
});

test('private upload invokes the real Git Bash executable directly on Windows', () => {
  const windows = warningArtifactCommand('store', env, 'win32', () => 'D:/alternate git/bin/bash.exe');
  assert.equal(windows.program, 'D:/alternate git/bin/bash.exe');
  assert.equal(windows.args[2], `12345/rust-warnings/${env.WARNING_FILE}`);
  assert.equal(windows.args[3], `ci-rust-warnings/${env.WARNING_FILE}`);
  assert.equal(warningArtifactCommand('store', env, 'linux').program, 'bash');
  assert.throws(() => warningArtifactCommand('store', { ...env, WARNING_FILE: '../other.json' }), /filename/);
  assert.throws(() => warningArtifactCommand('store', { ...env, GITHUB_RUN_ID: 'other/run' }), /operation\/run/);
});

test('composite uses stock shells and keeps warning evidence failure separate from the build result', () => {
  const action = parse(readFileSync(new URL('../../.github/actions/rust-warnings/action.yml', import.meta.url), 'utf8'));
  for (const step of action.runs.steps) {
    assert.equal(step.shell, "${{ runner.os == 'Windows' && 'pwsh' || 'bash' }}");
    assert.equal(step['continue-on-error'], true);
    assert.match(step.run, /^node scripts\/ci-rust-warning-artifact\.mjs (collect|store)$/);
  }
});
