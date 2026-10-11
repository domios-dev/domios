const { test } = require('node:test');
const assert = require('node:assert/strict');
const { guiEnvironment } = require('./gui-env.cjs');

test('GUI app environment drops shell tool paths and preload but preserves named DEV routing', () => {
  const env = guiEnvironment({ HOME: '/home/probe', USER: 'probe', PATH: '/nvm:/homebrew',
    NODE_OPTIONS: '--require=fixture', RL_GUI_MODE: '1', CODETERM_INSTANCE: 'mobileqa', CODETERM_API_PORT: '7690' });
  assert.deepEqual(env, { HOME: '/home/probe', USER: 'probe', PATH: '/usr/bin:/bin:/usr/sbin:/sbin',
    CODETERM_INSTANCE: 'mobileqa', CODETERM_API_PORT: '7690' });
});

test('preload sanitizes the actual app child rather than the tooling parent', () => {
  const { mkdtempSync, writeFileSync, chmodSync, rmSync } = require('node:fs');
  const { tmpdir } = require('node:os');
  const { join } = require('node:path');
  const { execFileSync } = require('node:child_process');
  const dir = mkdtempSync(join(tmpdir(), 'rl-gui-'));
  try {
    const app = join(dir, 'codeterm');
    writeFileSync(app, '#!/bin/sh\n/usr/bin/env\n');
    chmodSync(app, 0o755);
    const output = execFileSync(process.execPath, ['--require', require.resolve('./gui-env.cjs'), '-e',
      `require('node:child_process').spawn(${JSON.stringify(app)}, [], {stdio:'inherit', env:process.env})`],
      { encoding: 'utf8', env: { HOME: dir, USER: 'probe', PATH: '/nvm:/homebrew', RL_GUI_MODE: '1',
        CODETERM_INSTANCE: 'mobileqa', CODETERM_API_PORT: '7690' } });
    assert.match(output, /^PATH=\/usr\/bin:\/bin:\/usr\/sbin:\/sbin$/m);
    assert.match(output, /^CODETERM_INSTANCE=mobileqa$/m);
    assert.doesNotMatch(output, /RL_GUI_MODE|NODE_OPTIONS/);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
