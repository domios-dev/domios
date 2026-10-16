import test from 'node:test';
import assert from 'node:assert/strict';
import { captureBounded, parseBootPhase, phasesForTab } from '../../evals/startup-matrix/phase-collector.mjs';

test('a deadline persists incomplete evidence even when an operation ignores cancellation', async () => {
  const controller = new AbortController();
  const writes = [];
  let closed = 0;
  let entered = false;
  const capture = captureBounded({
    signal: controller.signal,
    collect: () => { entered = true; return new Promise(() => {}); },
    persist: result => writes.push(result),
    close: () => { closed++; },
  });
  await Promise.resolve();
  assert.equal(entered, true);
  controller.abort(new Error('capture deadline'));
  const result = await Promise.race([capture, Promise.resolve().then(() => 'still_waiting').then(value => value)]);
  assert.equal(result.status, 'incomplete');
  assert.match(result.error, /capture deadline/);
  assert.equal(writes.length, 1);
  assert.equal(closed, 1);
});

test('quoted phases and PTY-only first output retain their exact source timestamps', () => {
  const lines = [
    '[boot-phase] tab=own pty=p1 phase="pty_registered" at_us=1792116865807394',
    '[boot-phase] tab=- pty=p1 phase="first_output" at_us=1792116865824952',
    '[boot-phase] tab=other pty=p2 phase=AgentReady at_us=1792116871085821',
    '[boot-phase] tab=own pty=p1 phase=AgentReady at_us=1792116873458424',
  ];
  const own = phasesForTab(lines.map(parseBootPhase), 'own');
  assert.deepEqual(own.map(mark => mark.phase), ['pty_registered', 'first_output', 'AgentReady']);
  assert.equal(own[2].atUs - own[0].atUs, 7651030);
  assert.equal(parseBootPhase('unrelated diagnostic'), null);
});

test('a late operation cannot overwrite evidence after deadline termination', async () => {
  const controller = new AbortController();
  const writes = [];
  let complete;
  const operation = new Promise(resolve => { complete = resolve; });
  const captured = captureBounded({ signal: controller.signal, collect: () => operation, persist: value => writes.push(value) });
  await Promise.resolve();
  controller.abort('deadline');
  await captured;
  complete('late ready');
  await Promise.resolve();
  await Promise.resolve();
  assert.equal(writes.length, 1);
  assert.equal(writes[0].status, 'incomplete');
});

test('pre-aborted capture launches nothing and ordinary failure still writes evidence', async () => {
  const writes = [];
  let calls = 0;
  await captureBounded({ signal: AbortSignal.abort('deadline'), collect: () => { calls++; }, persist: value => writes.push(value) });
  assert.equal(calls, 0);
  await captureBounded({ signal: new AbortController().signal, collect: () => { throw Error('log rotated'); }, persist: value => writes.push(value) });
  assert.match(writes[1].error, /log rotated/);
});
