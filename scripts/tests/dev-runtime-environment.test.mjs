import test from 'node:test';
import assert from 'node:assert/strict';
import { join, resolve } from 'node:path';
import { devRuntimeEnvironment } from '../dev-runtime-environment.mjs';

test('runtime paths use the physical checkout after the build drive is released', () => {
  const physical = resolve('physical checkout');
  const inherited = {
    CODETERM_DEV_WORKSPACE_ROOT: 'X:/', CODETERM_WEB_DIST: 'X:/dist',
    CODETERM_CLI_PATH: '/instance/bin/codeterm', CODETERM_API_PORT: '7686',
  };
  const environment = devRuntimeEnvironment(inherited, physical);
  assert.equal(environment.CODETERM_DEV_WORKSPACE_ROOT, physical);
  assert.equal(environment.CODETERM_WEB_DIST, join(physical, 'dist'));
  assert.equal(environment.CODETERM_CLI_PATH, inherited.CODETERM_CLI_PATH);
  assert.equal(environment.CODETERM_API_PORT, '7686');
  assert.equal(inherited.CODETERM_WEB_DIST, 'X:/dist');
});
