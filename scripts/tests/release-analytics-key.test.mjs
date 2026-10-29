import assert from 'node:assert/strict';
import test from 'node:test';
import { amplitudeFingerprint, verifyAnalyticsKey } from '../check-release-analytics-key.mjs';

test('analytics fingerprint hashes exact nonblank key bytes', () => {
  assert.equal(amplitudeFingerprint('abc'), 'ba7816bf');
  for (const key of [undefined, '', ' \t\n']) assert.equal(amplitudeFingerprint(key), 'none');
  assert.notEqual(amplitudeFingerprint('abc'), amplitudeFingerprint(' abc'));
});

test('release guard accepts the expected fingerprint in the built server', () => {
  assert.equal(verifyAnalyticsKey(Buffer.from('\0domios-amplitude-prod:ba7816bf\0'), 'abc'), 'ba7816bf');
});

test('release guard refuses missing, stale or absent-key builds', () => {
  for (const content of ['', 'domios-amplitude-prod:00000000', 'domios-amplitude-prod:none',
    'domios-amplitude-prod:ba7816bf\0domios-amplitude-prod:none']) {
    assert.throws(() => verifyAnalyticsKey(Buffer.from(content), 'abc'), /differs/);
  }
  assert.throws(() => verifyAnalyticsKey(Buffer.from('domios-amplitude-prod:none'), undefined), /missing/);
});
