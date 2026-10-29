import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { execFileSync } from 'node:child_process';
import { checkFirebaseConfig } from '../check-firebase-config.mjs';
import { scanText } from '../check-public-privacy.mjs';

test('tracked Firebase keys fail both guards without disclosing their bytes', () => {
  const root = mkdtempSync(join(tmpdir(), 'firebase-guard-'));
  try {
    execFileSync('git', ['init', '-q', root]);
    writeFileSync(join(root, 'config.json'), '{"apiKey":"REPLACE_FIREBASE_API_KEY"}');
    execFileSync('git', ['add', 'config.json'], { cwd: root });
    assert.deepEqual(checkFirebaseConfig(root), []);
    const key = ['AI', 'za', 'fixture'.repeat(5)].join('');
    writeFileSync(join(root, 'config.json'), JSON.stringify({ apiKey: key }));
    assert.deepEqual(checkFirebaseConfig(root), ['config.json']);
    const findings = scanText('config.json', key);
    assert.equal(findings.length, 1);
    assert.equal(findings[0].rule, 'firebase-web-key');
    assert.ok(!JSON.stringify(findings).includes(key));
    assert.equal(checkFirebaseConfig(root).length, 1);
  } finally { rmSync(root, { recursive: true, force: true }); }
});
