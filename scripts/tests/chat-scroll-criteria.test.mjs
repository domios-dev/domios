import assert from 'node:assert/strict';
import test from 'node:test';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { isPaintedSample, paintedGapSamples } from '../qa/chat-scroll-criteria.mjs';

const sample = (edge, gap = 58) => ({ edge, gap });

test('manual measurements and unknown paint names are not painted samples', () => {
  for (const edge of ['installed', 'before-send', 'optimistic', 'echo', 'reply', 'before-arrival', 'arrival-reply', 'reload', 'unknown-paint']) {
    assert.equal(isPaintedSample(sample(edge)), false, edge);
  }
});

test('paint and declared transition edges detect real off-bottom geometry', () => {
  for (const edge of ['paint', 'inset-shrink-paint', 'inset-restore-paint', 'composer-expand-paint', 'composer-clear-paint', 'hydrated-reload-paint']) {
    const offBottom = sample(edge, 2);
    assert.equal(isPaintedSample(offBottom), true, edge);
    assert.deepEqual(paintedGapSamples([sample(edge, 0), sample(edge, 1), offBottom]), [offBottom]);
  }
});

test('streaming follow-up manual gap and first optimistic gap do not fail the send', () => {
  const samples = [sample('paint'), sample('before-send'), sample('optimistic'), sample('paint', 0),
    sample('before-send'), sample('paint', 0), sample('optimistic'), sample('echo'), sample('reply')];
  assert.deepEqual(paintedGapSamples(samples, { afterSend: true }), []);
});

test('actual paint and transitions after submission still fail', () => {
  const paint = sample('paint');
  const transition = sample('composer-expand-paint');
  assert.deepEqual(paintedGapSamples([sample('optimistic'), paint, sample('before-send'), transition], { afterSend: true }), [paint, transition]);
});

test('missing optimistic marker uses before-send boundary without suppressing paint failures', () => {
  const paint = sample('paint');
  assert.deepEqual(paintedGapSamples([sample('paint'), sample('before-send'), paint], { afterSend: true }), [paint]);
  assert.deepEqual(paintedGapSamples([paint], { afterSend: true }), [paint]);
});


test('offline wrapper exits zero only when corrected paint criteria and tester agree', () => {
  const dir = mkdtempSync(path.join(tmpdir(), 'chat-scroll-criteria-'));
  const reportPath = path.join(dir, 'report.json');
  const summaryPath = path.join(dir, 'summary.json');
  const result = { label: 'streaming-0', phase: 'streaming', errors: ['1 painted samples outside bottom'],
    samples: [sample('optimistic'), sample('before-send'), sample('paint', 0)] };
  const report = { cases: [result], failures: [result.label] };
  const summary = { cases: [{ label: result.label, verdict: 'PASS', maxPaintedGap: 0 }] };
  const check = () => {
    writeFileSync(reportPath, JSON.stringify(report));
    writeFileSync(summaryPath, JSON.stringify(summary));
    return spawnSync('bash', ['scripts/qa/chat-live-scroll.sh', '--check-report', reportPath, summaryPath], { encoding: 'utf8' });
  };
  try {
    assert.equal(check().status, 0);
    result.samples.push(sample('paint'));
    assert.equal(check().status, 1);
    result.samples.pop();
    result.errors.push('Missing live DOM turn');
    assert.equal(check().status, 1);
    result.errors.pop();
    summary.cases[0].verdict = 'FAIL';
    assert.equal(check().status, 1);
    summary.cases[0].verdict = 'PASS';
    report.error = 'Harness aborted';
    assert.equal(check().status, 1);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
