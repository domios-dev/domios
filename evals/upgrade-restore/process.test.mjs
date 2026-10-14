import {test} from 'node:test';
import assert from 'node:assert/strict';
import {EventEmitter} from 'node:events';
import {PassThrough} from 'node:stream';
import {run} from './process.mjs';

function harness(maxBuffer = 1024) {
  const child = new EventEmitter();
  child.pid = 42;
  child.stdout = new PassThrough();
  child.stderr = new PassThrough();
  const controller = new AbortController();
  const stopped = [];
  let spawnOptions;
  const result = run('fixture', [], {signal: controller.signal, maxBuffer,
    spawn: (_command, _args, options) => {spawnOptions = options; return child;},
    stop: target => stopped.push(target.pid)});
  return {child, controller, stopped, result, spawnOptions};
}

test('process completion preserves UTF-8 across chunks and requests an isolated group', async () => {
  const fixture = harness();
  const bytes = Buffer.from('□');
  fixture.child.stdout.write(bytes.subarray(0, 1));
  fixture.child.stdout.write(bytes.subarray(1));
  fixture.child.emit('close', 0);
  assert.equal((await fixture.result).stdout, '□');
  assert.equal(fixture.spawnOptions.detached, process.platform !== 'win32');
  fixture.controller.abort();
  assert.deepEqual(fixture.stopped, []);
});

test('deadline rejects immediately and stops the whole child group even without pipe EOF', async () => {
  const fixture = harness();
  fixture.controller.abort();
  await assert.rejects(fixture.result, error => error.killed === true);
  assert.deepEqual(fixture.stopped, [42]);
});

test('output cap fails closed and preserves structured nonzero CLI output', async () => {
  const bounded = harness(1);
  bounded.child.stdout.write('xx');
  await assert.rejects(bounded.result, /capture bound/);
  assert.deepEqual(bounded.stopped, [42]);
  const failed = harness();
  failed.child.stdout.write('{"ok":false}');
  failed.child.emit('close', 1);
  await assert.rejects(failed.result, error => error.stdout === '{"ok":false}');
});
