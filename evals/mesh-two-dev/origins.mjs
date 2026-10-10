import { verifyPair } from './identity.mjs';
import { createHash } from 'node:crypto';
import { validateTarget } from './sampling.mjs';
export function originPlan(origin, ids, fixture) {
  const home = origin === 'aiderqa' ? 'meshqa' : origin === 'meshqa' ? 'aiderqa' : null;
  if (!home || !ids[home] || !ids[origin]) throw new Error('Pinned origin/home identities required');
  const mesh = ['--mesh', ids[home]];
  const tab = ['--tab', fixture.tabId];
  if (!fixture.tabId || !fixture.sessionId || !fixture.payloadPad || !fixture.reportPad || !fixture.payloadDigest) throw new Error('Bound fixture and pad bodies required');
  return [
    { op: 'tab-list', args: ['tab', 'list', '--json', ...mesh] },
    { op: 'send', args: ['send', ...tab, '--require-agent', '--from-pad', fixture.payloadPad, '--json', ...mesh] },
    { op: 'get-text', args: ['tab', 'get-text', ...tab, '--json', ...mesh] },
    { op: 'paste-put', args: ['paste', 'put', '--from-pad', fixture.payloadPad, '--json', ...mesh] },
    { op: 'paste-get', args: ['paste', 'get', '@paste-put', ...mesh] },
    { op: 'report-request', args: ['send', ...tab, '--require-agent', '--from-pad', fixture.reportPad, '--json', ...mesh] },
  ].map(step => ({ ...step, origin, home, originPeerId: ids[origin], targetPeerId: ids[home], tabId: fixture.tabId, sessionId: fixture.sessionId, payloadDigest: fixture.payloadDigest }));
}
export async function driveOrigin(plan, adapter, write, signal) {
  const results = [];
  let pasteId;
  for (const planned of plan) {
    const step = { ...planned, args: planned.args.map(arg => arg === '@paste-put' ? pasteId : arg) };
    if (step.args.some(arg => !arg)) throw new Error('Paste put did not return an identity');
    signal.throwIfAborted();
    let result;
    try {
      result = await adapter(step, AbortSignal.any([signal, AbortSignal.timeout(20000)]));
      validateTarget(result.originName, result.originPort);
      if (result.originName !== step.origin || result.originPeerId !== step.originPeerId || result.answeredBy !== step.targetPeerId) throw new Error('Unproven command origin or target');
      if (result.code !== 0) throw new Error(`Command failed: ${result.code}`);
      if (step.op === 'paste-put') {
        pasteId = result.pasteId;
        if (!pasteId) throw new Error('Paste put did not return an identity');
      }
      if (step.op === 'paste-get' && createHash('sha256').update(result.body ?? '').digest('hex') !== step.payloadDigest) throw new Error('Paste body changed across mesh');
    } catch (error) {
      await write({ step, result, error: error.message, status: 'FAIL' });
      throw error;
    }
    const evidence = await write({ step, result, status: 'CAPTURED' });
    results.push({ step, result, evidence });
  }
  return results;
}
export function verifyReport(receipt, expected) {
  if (!receipt.persisted || !receipt.visible || receipt.originPeerId !== expected.originPeerId || receipt.senderPeerId !== expected.homePeerId || receipt.tabId !== expected.tabId || receipt.sessionId !== expected.sessionId || receipt.body !== expected.body) throw new Error('Report did not persist visibly at the origin');
  return true;
}
export async function driveBoth({ endpoints, ids, fixtures, adapters, write, signal }) {
  verifyPair(endpoints, ids);
  const results = {};
  for (const origin of ['aiderqa', 'meshqa']) {
    results[origin] = await driveOrigin(originPlan(origin, ids, fixtures[origin]), adapters[origin], write, signal);
  }
  return results;
}
