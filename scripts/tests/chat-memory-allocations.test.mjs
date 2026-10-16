import {test} from 'node:test';
import assert from 'node:assert/strict';
import {summarizeAllocations} from '../../evals/perf/chat-memory/allocations.mjs';

test('allocation summary separates own volume from callers without counting recursion twice', () => {
  const node=(name,size,children=[])=>({callFrame:{functionName:name,url:'fixture.js',lineNumber:0,columnNumber:2},selfSize:size,children});
  const summary=summarizeAllocations({head:node('root',0,[node('parse',10,[node('parse',20)]),node('render',40)]),samples:[{},{}]});
  assert.equal(summary.estimatedAllocatedBytes,70);
  assert.equal(summary.sampleCount,2);
  assert.equal(summary.self.find(x=>x.frame.startsWith('parse ')).bytes,30);
  assert.equal(summary.inclusive.find(x=>x.frame.startsWith('parse ')).bytes,30);
  assert.equal(summary.inclusive.find(x=>x.frame.startsWith('root ')).bytes,70);
  assert.equal(summary.self[0].bytes,40);
});
