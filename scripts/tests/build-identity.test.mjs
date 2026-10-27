import assert from 'node:assert/strict';
import test from 'node:test';
import { isDevelopmentFrontendBuild } from '../build-identity.cjs';

test('frontend development identity requires serving or an explicit frontend dev build', () => {
  assert.equal(isDevelopmentFrontendBuild('serve', {}), true);
  assert.equal(isDevelopmentFrontendBuild('build', { CODETERM_FRONTEND_DEV: '1' }), true);
  assert.equal(isDevelopmentFrontendBuild('build', {}), false);
  assert.equal(isDevelopmentFrontendBuild('build', { CODETERM_DEV: '1' }), false);
  assert.equal(isDevelopmentFrontendBuild('build', { CODETERM_FRONTEND_DEV: '0' }), false);
});
