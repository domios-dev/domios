import test from 'node:test';
import assert from 'node:assert/strict';
import {memoryInstrumentation} from './instrumentation.mjs';

test('minimal control refuses profiling instead of silently changing the scenario', () => {
  assert.deepEqual(memoryInstrumentation({}), {mode:'full',profiler:true});
  assert.deepEqual(memoryInstrumentation({instrumentation:'minimal',snapshots:'no'}), {mode:'minimal',profiler:false});
  assert.throws(() => memoryInstrumentation({instrumentation:'minimal',snapshots:'yes'}));
  assert.throws(() => memoryInstrumentation({instrumentation:'minimal',allocations:'yes'}));
  assert.throws(() => memoryInstrumentation({instrumentation:'unknown'}));
});
