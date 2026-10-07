import assert from 'node:assert/strict';
import { commands, decision, followUp, quote, verdict } from './model.mjs';
import { completedTurn } from './turn.mjs';

let count = 0;
function equal(actual: unknown, expected: unknown) { assert.deepEqual(actual, expected); count++; }
const start = { kind: 'adapter_start', sessionId: 's', processGeneration: 'g' };
const done = { ...start, kind: 'turn_complete', turnSequence: 1, historyBytes: 12, outcome: 'answered', complete: true };
const text = (record: object) => `${JSON.stringify(start)}\n${JSON.stringify(record)}\n`;
const first = completedTurn(text(done), 's', 'g', 12);
equal(first, { key: 's:g:1', outcome: 'answered', complete: true });
equal(completedTurn(text(done), 's', 'g', 13), null);
equal(completedTurn(JSON.stringify(start) + '\n', 's', 'g', 12), null);
equal(completedTurn(text(done).slice(0, -1), 's', 'g', 12), null);
equal(completedTurn(text({ ...done, turnSequence: 2 }), 's', 'g', 12)?.key, 's:g:2');
for (const outcome of ['error', 'cancelled', 'reflection_limit']) equal(completedTurn(text({ ...done, outcome, complete: false }), 's', 'g', 12)?.complete, false);
assert.throws(() => completedTurn(text(done), 'other', 'g', 12)); count++;
equal(decision({ active: false, claimed: [] }, first.key), 'ready');
equal(decision({ active: false, claimed: [first.key] }, first.key), 'waiting');
equal(decision({ active: true, claimed: [] }, first.key), 'waiting');
equal(followUp(first.key, first.key), false);
equal(followUp(first.key, `${first.key}-next`), true);
equal(followUp(first.key, undefined), false);
assert.throws(() => commands('unknown')); count++;
for (const surface of ['config', 'defaults', 'sdk', 'shortcuts']) equal(commands(surface)[0], ['npx', 'tsc', '--noEmit']);
equal(verdict([{ code: 0 }]), 'PASSED');
for (const result of [{ code: 1 }, { code: null }, { code: 0, timeout: true }, { code: 0, signal: 'SIGTERM' }, { code: 0, error: 'removed recipient' }]) equal(verdict([result]), 'FAILED');
equal(verdict([]), 'FAILED');
equal(quote("D:/lane's space", 'win32'), "'D:/lane''s space'");
equal(quote('exact-id', 'linux'), "'exact-id'");
console.log(`lane-check model: ${count} assertions passed`);

const { EventEmitter } = await import('node:events');
const { run } = await import('./process.mjs');
async function processCase(code: number | null, timeout: boolean, killCode = 0) {
  const child = Object.assign(new EventEmitter(), { stdout: new EventEmitter(), stderr: new EventEmitter(), pid: 123 });
  let expire: (() => void) | undefined;
  const calls: unknown[][] = [];
  const result = run(['node', 'path with spaces', 'exact-id'], 'D:/lane space', 100, {
    platform: 'win32',
    spawn: (...args: unknown[]) => {
      calls.push(args);
      if (calls.length === 1) return child;
      const killer = new EventEmitter();
      queueMicrotask(() => killer.emit('close', killCode));
      return killer;
    },
    arm: (callback: () => void) => { expire = callback; return 1; },
    disarm: () => {},
  });
  if (timeout) expire!();
  child.emit('close', code, timeout ? 'SIGTERM' : null);
  return { result: await result, calls };
}
const passed = await processCase(0, false);
equal(verdict([passed.result]), 'PASSED');
const failed = await processCase(1, false);
equal(verdict([failed.result]), 'FAILED');
const timed = await processCase(null, true);
equal(verdict([timed.result]), 'FAILED');
equal(timed.calls[1][1], ['/PID', '123', '/T', '/F']);
const uncertain = await processCase(null, true, 1);
equal(uncertain.result.contained, false);
equal(verdict([uncertain.result]), 'FAILED');
const encoded = (passed.calls[0][1] as string[])[3];
equal(Buffer.from(encoded, 'base64').toString('utf16le'), "& 'node' 'path with spaces' 'exact-id'");
console.log(`lane-check total: ${count} assertions passed`);

const { mkdtempSync, writeFileSync, rmSync } = await import('node:fs');
const { tmpdir } = await import('node:os');
const path = await import('node:path');
const { execFileSync } = await import('node:child_process');
const { fingerprint } = await import('./runtime.mjs');
const fixture = mkdtempSync(path.join(tmpdir(), 'lane check '));
try {
  const git = (...args: string[]) => execFileSync('git', args, { cwd: fixture, stdio: 'pipe' });
  git('init');
  git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '--allow-empty', '-m', 'fixture');
  writeFileSync(path.join(fixture, 'same file.txt'), 'one');
  const hash1 = fingerprint(fixture);
  equal(fingerprint(fixture), hash1);
  writeFileSync(path.join(fixture, 'same file.txt'), 'two');
  assert.notEqual(fingerprint(fixture), hash1); count++;
  git('add', 'same file.txt');
  git('-c', 'user.name=Fixture', '-c', 'user.email=fixture@example.invalid', 'commit', '-m', 'tracked');
  const hash2 = fingerprint(fixture);
  writeFileSync(path.join(fixture, 'same file.txt'), 'three');
  assert.notEqual(fingerprint(fixture), hash2); count++;
} finally { rmSync(fixture, { recursive: true, force: true }); }
console.log(`lane-check complete: ${count} assertions passed`);

const { missingTarget } = await import('./model.mjs');
equal(commands('automation'), [
  ['npx', 'tsc', '--noEmit'],
  ['node', 'scripts/run-ts-unit-tests.mjs', 'src/components/Automation/nodeDetailsModel.test.ts', 'src/components/Automation/CommandResultDetails.test.tsx', 'src/components/Automation/AutomationGraphView.test.tsx'],
]);
equal(missingTarget(commands('automation')[1], file => !file.endsWith('nodeDetailsModel.test.ts')), 'src/components/Automation/nodeDetailsModel.test.ts');
equal(missingTarget(commands('automation')[1], file => !file.endsWith('AutomationGraphView.test.tsx')), 'src/components/Automation/AutomationGraphView.test.tsx');
equal(missingTarget(commands('automation')[1], () => true), undefined);
equal(decision({ active: false, claimed: ['turn-1'], lastHash: 'hash-1' }, 'turn-2', 'hash-1'), 'waiting');
equal(decision({ active: false, claimed: ['turn-1'], lastHash: 'hash-1' }, 'turn-2', 'hash-2'), 'ready');
console.log(`lane-check final: ${count} assertions passed`);

const { blankUserRecord } = await import('./turn.mjs');
equal(blankUserRecord('\n#### <blank>  \n'), true);
equal(blankUserRecord('####  \n'), true);
equal(blankUserRecord('#### actual task\n'), false);
const blank = { ...done, turnSequence: 2, historyBytes: 28 };
const withBlank = `${JSON.stringify(start)}\n${JSON.stringify(done)}\n${JSON.stringify(blank)}\n`;
equal(completedTurn(withBlank, 's', 'g', 28, record => record.turnSequence === 2 ? '#### <blank>  \n' : '#### task\n')?.key, 's:g:1');
equal(completedTurn(text(blank), 's', 'g', 28, () => '#### <blank>  \n'), null);
equal(completedTurn(withBlank, 's', 'g', 29, () => '#### task\n'), null);
console.log(`lane-check blank filtering: ${count} assertions passed`);

const { deliver, diagnosticResults } = await import('./verdict.mjs');
equal(diagnosticResults([{ code: 0, output: '#< CLIXML\nmodule noise' }]), [{ code: 0 }]);
equal(diagnosticResults([{ code: 1, output: '#< CLIXML\n<Objs><Obj S="progress">noise</Obj></Objs>\nactual error' }]), [{ code: 1, output: 'actual error' }]);
const sent: unknown[][] = [], written: unknown[][] = [], persisted: unknown[][] = [];
const routes = deliver('dev-cli', { worker: 'worker-exact', orchestrator: 'mozart-exact', lane: 'D:/lane space' }, 'D:/state', 'opaque verdict content', {
  call: (...args: unknown[]) => {
    if ((args[1] as string[])[0] === 'pad') return 'D:/pad space';
    sent.push(args);
    if ((args[1] as string[])[2] === 'worker-exact') throw new Error('recipient removed');
    return 'OK (sent)';
  },
  write: (...args: unknown[]) => written.push(args),
  save: (...args: unknown[]) => persisted.push(args),
});
equal(sent.length, 2);
equal(sent.map(args => args[3]), [30000, 30000]);
equal((sent[0][1] as string[]).slice(0, 5), ['send', '--tab', 'worker-exact', '--from-pad', 'lane-verdict-worker-exact.txt']);
equal(written.map(args => args[1]), ['opaque verdict content', 'opaque verdict content']);
equal(routes[0].unconfirmed, 'Error: recipient removed');
equal(routes[1].result, 'OK (sent)');
equal(persisted.length, 2);
console.log(`lane-check routing: ${count} assertions passed`);

const timeoutCalls: unknown[][] = [];
const timedRoutes = deliver('dev-cli', { worker: 'worker', orchestrator: 'mozart', lane: 'lane' }, 'state', 'payload', {
  call: (...args: unknown[]) => {
    if ((args[1] as string[])[0] === 'pad') return 'pad';
    timeoutCalls.push(args);
    throw new Error('ETIMEDOUT after possible write');
  },
  write: () => {}, save: () => {},
});
equal(timeoutCalls.length, 2);
equal(timedRoutes.every(route => route.unconfirmed.includes('ETIMEDOUT')), true);
console.log(`lane-check verified: ${count} assertions passed`);
